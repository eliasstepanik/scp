use scp_core::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use serde_json::{json, Value};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Router error types
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum RouterError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Pool error: {0}")]
    PoolError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

/// Router handles request routing and fan-out
#[allow(dead_code)]
pub struct Router {
    pool_manager: Arc<PoolManager>,
    tool_registry: Arc<RwLock<ToolRegistry>>,
    fanout_timeout_secs: u64,
    request_token_budget: usize,
}

impl Router {
    #[allow(dead_code)]
    /// Create a new router
    pub fn new(
        pool_manager: Arc<PoolManager>,
        tool_registry: Arc<RwLock<ToolRegistry>>,
        fanout_timeout_secs: u64,
        request_token_budget: usize,
    ) -> Self {
        Self {
            pool_manager,
            tool_registry,
            fanout_timeout_secs,
            request_token_budget,
        }
    }

    /// Route a request to the appropriate backend
    #[allow(dead_code)]
    pub async fn route(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "ping" => self.handle_ping(&request),
            "tools/list" => self.handle_tools_list(&request).await,
            "tools/call" => self.handle_tools_call(&request).await,
            "initialize" => self.handle_initialize(&request).await,
            _ => self.handle_unknown(&request),
        }
    }

    /// Handle ping request (respond directly)
    #[allow(dead_code)]
    fn handle_ping(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling ping request");
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({})),
            error: None,
        }
    }

    /// Handle tools/list request (fan-out to all servers)
    async fn handle_tools_list(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling tools/list request");

        // Get all servers
        let servers = self.pool_manager.list_servers().await;

        if servers.is_empty() {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(json!({ "tools": [] })),
                error: None,
            };
        }

        // For now, return empty tools list (full implementation in P1.E.3)
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({ "tools": [] })),
            error: None,
        }
    }

    /// Handle tools/call request (route to specific server)
    async fn handle_tools_call(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling tools/call request");

        // Extract tool name from params
        let tool_name = match &request.params {
            Some(Value::Object(obj)) => match obj.get("name") {
                Some(Value::String(name)) => name.clone(),
                _ => {
                    return self.error_response(
                        request.id.clone(),
                        JsonRpcError::INVALID_PARAMS,
                        "Missing or invalid 'name' parameter",
                    );
                }
            },
            _ => {
                return self.error_response(
                    request.id.clone(),
                    JsonRpcError::INVALID_PARAMS,
                    "Invalid params",
                );
            }
        };

        // Lookup tool in registry
        let registry = self.tool_registry.read().await;
        match registry.lookup(&tool_name) {
            Some(_tool_entry) => {
                drop(registry);

                // For now, return error (full implementation in P1.E.2)
                self.error_response(
                    request.id.clone(),
                    JsonRpcError::BACKEND_ERROR,
                    "Server not available",
                )
            }
            None => {
                drop(registry);
                self.error_response(
                    request.id.clone(),
                    JsonRpcError::INVALID_PARAMS,
                    format!("Tool not found: {}", tool_name),
                )
            }
        }
    }

    /// Handle initialize request (fan-out to all servers)
    async fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling initialize request");

        // For now, return basic capabilities (full implementation in P1.E.5)
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "serverInfo": {
                    "name": "scp",
                    "version": "0.2.0"
                }
            })),
            error: None,
        }
    }

    /// Handle unknown request
    fn handle_unknown(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        warn!("Unknown method: {}", request.method);
        self.error_response(
            request.id.clone(),
            JsonRpcError::METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        )
    }

    /// Create an error response
    fn error_response(
        &self,
        id: Option<RequestId>,
        code: i32,
        message: impl Into<String>,
    ) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError::new(code, message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_router_ping() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "ping".to_string(),
            params: None,
        };

        let response = router.route(request).await;
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_router_unknown_method() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = router.route(request).await;
        assert!(response.error.is_some());
    }
}
