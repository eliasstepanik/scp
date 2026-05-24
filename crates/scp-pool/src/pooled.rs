use crate::PoolError;
use scp_core::protocol::{IncomingMessage, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_transport::stdio_server::StdioServerTransport;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Pool worker with transport and in-flight request tracking
pub struct PoolWorker {
    pub transport: Arc<Mutex<StdioServerTransport>>,
    pub in_flight: Arc<AtomicUsize>,
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    _receive_loop: JoinHandle<()>,
}

impl PoolWorker {
    /// Create a new pool worker
    pub fn new(transport: StdioServerTransport) -> Self {
        let transport = Arc::new(Mutex::new(transport));
        let pending = Arc::new(Mutex::new(HashMap::new()));

        // Spawn receive loop
        let transport_clone = transport.clone();
        let pending_clone = pending.clone();
        let receive_loop = tokio::spawn(async move {
            Self::receive_loop_task(transport_clone, pending_clone).await;
        });

        Self {
            transport,
            in_flight: Arc::new(AtomicUsize::new(0)),
            pending,
            _receive_loop: receive_loop,
        }
    }

    /// Get current in-flight request count
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Increment in-flight counter
    pub fn increment_in_flight(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement in-flight counter
    pub fn decrement_in_flight(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    /// Background receive loop task
    async fn receive_loop_task(
        transport: Arc<Mutex<StdioServerTransport>>,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    ) {
        loop {
            let msg = {
                let mut transport = transport.lock().await;
                match transport.receive().await {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!("Transport receive error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            };

            // Only process responses
            if let Some(IncomingMessage::Response(response)) = msg {
                // Extract internal ID from response
                if let Some(RequestId::String(id)) = &response.id {
                    let mut pending = pending.lock().await;
                    if let Some(tx) = pending.remove(id) {
                        debug!("Dispatching response for request: {}", id);
                        let _ = tx.send(response);
                    } else {
                        warn!("Received response for unknown request ID: {}", id);
                    }
                }
            }
        }
    }
}

/// Pooled pool strategy — N backend connections with least-outstanding-requests dispatch
pub struct PooledPool {
    workers: Vec<Arc<PoolWorker>>,
    #[allow(dead_code)]
    next_worker_idx: Arc<Mutex<usize>>,
    max_queue_depth: usize,
}

impl PooledPool {
    /// Create a new pooled pool
    pub fn new(max_queue_depth: usize) -> Self {
        Self {
            workers: Vec::new(),
            next_worker_idx: Arc::new(Mutex::new(0)),
            max_queue_depth,
        }
    }

    /// Add a worker to the pool
    pub fn add_worker(&mut self, transport: StdioServerTransport) {
        let worker = Arc::new(PoolWorker::new(transport));
        self.workers.push(worker);
        info!(
            "Added worker to pooled pool, total workers: {}",
            self.workers.len()
        );
    }

    /// Get the worker with the least outstanding requests
    fn get_least_busy_worker(&self) -> Option<Arc<PoolWorker>> {
        self.workers
            .iter()
            .min_by_key(|w| w.in_flight_count())
            .cloned()
    }

    /// Send a request to the least busy worker
    pub async fn send_request(
        &self,
        mut request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, PoolError> {
        if self.workers.is_empty() {
            return Err(PoolError::ServerNotAvailable);
        }

        let worker = self
            .get_least_busy_worker()
            .ok_or(PoolError::ServerNotAvailable)?;

        // Check queue depth
        if worker.in_flight_count() >= self.max_queue_depth {
            return Err(PoolError::Internal("Pool queue depth exceeded".to_string()));
        }

        // Generate internal request ID
        let internal_id = format!("scp-{}-{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        let original_id = request.id.clone();

        // Store original ID for mapping back
        request.id = Some(RequestId::String(internal_id.clone()));

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = worker.pending.lock().await;
            pending.insert(internal_id.clone(), tx);
        }

        worker.increment_in_flight();

        // Send request
        {
            let transport = worker.transport.lock().await;
            let request_value =
                serde_json::to_value(&request).map_err(|e| PoolError::Internal(e.to_string()))?;
            transport
                .send(&request_value)
                .await
                .map_err(|e| PoolError::TransportError(e.to_string()))?;
        }

        debug!("Request sent to worker with internal ID: {}", internal_id);

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| PoolError::Timeout)?
            .map_err(|_| PoolError::Cancelled)?;

        worker.decrement_in_flight();

        // Map response ID back to original
        let mut mapped_response = response;
        mapped_response.id = original_id;

        Ok(mapped_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pooled_pool_creation() {
        let pool = PooledPool::new(10);
        assert_eq!(pool.workers.len(), 0);
    }

    #[test]
    #[ignore]
    fn test_pool_worker_in_flight() {
        // Skipped: requires process spawning which is tested in integration tests
    }
}
