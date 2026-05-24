use crate::PoolError;
use scp_core::protocol::{IncomingMessage, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_transport::stdio_server::StdioServerTransport;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, warn};
use uuid::Uuid;

/// Session ID type
pub type SessionId = String;

/// Session entry with transport and pending requests
struct SessionEntry {
    transport: Arc<Mutex<StdioServerTransport>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    _receive_loop: JoinHandle<()>,
}

/// Dedicated pool strategy — one backend per session
pub struct DedicatedPool {
    sessions: Arc<Mutex<HashMap<SessionId, SessionEntry>>>,
}

impl DedicatedPool {
    /// Create a new dedicated pool
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get or create a transport for a session
    pub async fn get_or_create(
        &self,
        session_id: &SessionId,
        create_fn: impl Fn() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<StdioServerTransport, PoolError>>>,
        >,
    ) -> Result<Arc<Mutex<StdioServerTransport>>, PoolError> {
        let mut sessions = self.sessions.lock().await;

        if let Some(entry) = sessions.get(session_id) {
            return Ok(entry.transport.clone());
        }

        // Create new transport
        let transport = create_fn().await?;
        let transport_arc = Arc::new(Mutex::new(transport));
        let pending = Arc::new(Mutex::new(HashMap::new()));

        // Spawn receive loop
        let transport_clone = transport_arc.clone();
        let pending_clone = pending.clone();
        let receive_loop = tokio::spawn(async move {
            Self::receive_loop_task(transport_clone, pending_clone).await;
        });

        let entry = SessionEntry {
            transport: transport_arc.clone(),
            pending,
            _receive_loop: receive_loop,
        };

        sessions.insert(session_id.clone(), entry);

        debug!("Created dedicated backend for session: {}", session_id);
        Ok(transport_arc)
    }

    /// Remove a session's backend connection
    pub async fn remove_session(&self, session_id: &SessionId) -> bool {
        let mut sessions = self.sessions.lock().await;
        let removed = sessions.remove(session_id).is_some();
        if removed {
            debug!("Removed dedicated backend for session: {}", session_id);
        }
        removed
    }

    /// Send a request to a session's backend
    pub async fn send_request(
        &self,
        session_id: &SessionId,
        mut request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, PoolError> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(session_id)
            .ok_or(PoolError::ServerNotAvailable)?;

        // Generate internal request ID
        let internal_id = format!("scp-{}-{}", Uuid::new_v4(), Uuid::new_v4());
        let original_id = request.id.clone();

        // Store original ID for mapping back
        request.id = Some(RequestId::String(internal_id.clone()));

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = entry.pending.lock().await;
            pending.insert(internal_id.clone(), tx);
        }

        // Send request
        {
            let transport = entry.transport.lock().await;
            let request_value =
                serde_json::to_value(&request).map_err(|e| PoolError::Internal(e.to_string()))?;
            transport
                .send(&request_value)
                .await
                .map_err(|e| PoolError::TransportError(e.to_string()))?;
        }

        drop(sessions);

        debug!("Request sent to session {} with internal ID: {}", session_id, internal_id);

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| PoolError::Timeout)?
            .map_err(|_| PoolError::Cancelled)?;

        // Map response ID back to original
        let mut mapped_response = response;
        mapped_response.id = original_id;

        Ok(mapped_response)
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

impl Default for DedicatedPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dedicated_pool_creation() {
        let pool = DedicatedPool::new();
        assert_eq!(pool.sessions.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn test_remove_session() {
        let pool = DedicatedPool::new();
        let session_id = "test-session".to_string();

        // Try to remove non-existent session
        let removed = pool.remove_session(&session_id).await;
        assert!(!removed);
    }
}
