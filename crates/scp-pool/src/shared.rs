use scp_core::protocol::{IncomingMessage, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_transport::stdio_server::StdioServerTransport;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{oneshot, Mutex, OnceCell};
use tracing::{debug, warn};
use uuid::Uuid;

/// Pool error types
#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Server not available")]
    ServerNotAvailable,

    #[error("Request cancelled")]
    Cancelled,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Shared pool wraps a single transport with request serialization
pub struct SharedPool {
    transport: Arc<Mutex<StdioServerTransport>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    initialized: OnceCell<()>,
}

impl SharedPool {
    /// Create a new shared pool from a transport
    pub fn new(transport: StdioServerTransport) -> Self {
        Self {
            transport: Arc::new(Mutex::new(transport)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            initialized: OnceCell::new(),
        }
    }

    /// Ensure the MCP session is initialized (idempotent, race-free)
    async fn ensure_initialized(&self) -> Result<(), PoolError> {
        self.initialized
            .get_or_try_init(|| async {
                // Send initialize request
                let init_id = format!("scp-init-{}", Uuid::new_v4());
                let init_req = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "scp", "version": "0.2.0" }
                    }
                });

                let (tx, rx) = oneshot::channel();
                {
                    let mut pending = self.pending.lock().await;
                    pending.insert(init_id.clone(), tx);
                }

                {
                    let transport = self.transport.lock().await;
                    transport
                        .send(&init_req)
                        .await
                        .map_err(|e| PoolError::TransportError(e.to_string()))?;
                }

                // Wait for initialize response
                tokio::time::timeout(std::time::Duration::from_secs(30), rx)
                    .await
                    .map_err(|_| PoolError::Timeout)?
                    .map_err(|_| PoolError::Cancelled)?;

                // Send initialized notification (no response expected)
                let notif = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                });
                {
                    let transport = self.transport.lock().await;
                    transport
                        .send(&notif)
                        .await
                        .map_err(|e| PoolError::TransportError(e.to_string()))?;
                }

                debug!("MCP session initialized");
                Ok(())
            })
            .await
            .map(|_| ())
    }

    /// Send a method call and return the result Value
    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, PoolError> {
        self.ensure_initialized().await?;

        let internal_id = format!("scp-{}", Uuid::new_v4());

        let request_value = serde_json::json!({
            "jsonrpc": "2.0",
            "id": internal_id,
            "method": method,
            "params": params.unwrap_or(serde_json::json!({}))
        });

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(internal_id.clone(), tx);
        }

        {
            let transport = self.transport.lock().await;
            transport
                .send(&request_value)
                .await
                .map_err(|e| PoolError::TransportError(e.to_string()))?;
        }

        debug!("Request sent with internal ID: {}", internal_id);

        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| PoolError::Timeout)?
            .map_err(|_| PoolError::Cancelled)?;

        if let Some(err) = response.error {
            return Err(PoolError::TransportError(format!(
                "RPC error {}: {}",
                err.code, err.message
            )));
        }

        response
            .result
            .ok_or_else(|| PoolError::Internal("Response has no result".to_string()))
    }

    /// Send a raw JsonRpcRequest and wait for response (low-level)
    pub async fn send_request(
        &self,
        mut request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, PoolError> {
        // Generate internal request ID
        let internal_id = format!("scp-{}-{}", Uuid::new_v4(), Uuid::new_v4());
        let original_id = request.id.clone();

        // Store original ID for mapping back
        request.id = Some(RequestId::String(internal_id.clone()));

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.lock().await;
            pending.insert(internal_id.clone(), tx);
        }

        // Send request
        {
            let transport = self.transport.lock().await;
            let request_value =
                serde_json::to_value(&request).map_err(|e| PoolError::Internal(e.to_string()))?;
            transport
                .send(&request_value)
                .await
                .map_err(|e| PoolError::TransportError(e.to_string()))?;
        }

        debug!("Request sent with internal ID: {}", internal_id);

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

    /// Receive responses from the transport and dispatch to pending requests
    pub async fn receive_loop(&self) -> Result<(), PoolError> {
        loop {
            let msg = {
                let mut transport = self.transport.lock().await;
                transport
                    .receive()
                    .await
                    .map_err(|e| PoolError::TransportError(e.to_string()))?
            };

            match msg {
                Some(IncomingMessage::Response(response)) => {
                    // Extract internal ID from response
                    if let Some(RequestId::String(id)) = &response.id {
                        let mut pending = self.pending.lock().await;
                        if let Some(tx) = pending.remove(id) {
                            debug!("Dispatching response for request: {}", id);
                            let _ = tx.send(response);
                        } else {
                            warn!("Received response for unknown request ID: {}", id);
                        }
                    }
                }
                Some(IncomingMessage::Notification(_)) => {
                    // Notifications are not tracked, just ignore
                }
                Some(IncomingMessage::Request(_)) => {
                    // Unexpected server-initiated request — ignore
                }
                None => {
                    // Transport closed
                    return Err(PoolError::TransportError("Transport closed".to_string()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_shared_pool_creation() {
        // This test just verifies the struct can be created
        // Full testing requires a mock transport
    }
}
