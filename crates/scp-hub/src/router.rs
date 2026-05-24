use scp_core::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_index::{ToolEntry, ToolRegistry};
use scp_pool::PoolManager;
use scp_transport::http_server::HttpServerTransport;
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

        // SCP extension tools — always present and always first
        let extension_tools: Vec<Value> = vec![
            json!({
                "name": "scp_get_more",
                "description": "Retrieve additional filtered content from a previous response",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "request_id": {
                            "type": "string",
                            "description": "The request ID from a previous response"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Offset for pagination"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of items to return"
                        }
                    },
                    "required": ["request_id"]
                }
            }),
            json!({
                "name": "scp_info",
                "description": "Get information about the SCP hub",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            json!({
                "name": "scp_budget",
                "description": "Get current token budget status",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            json!({
                "name": "scp_budget_reset",
                "description": "Reset the current session token budget",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
        ];

        // Fan-out to all available backend servers in parallel
        let server_configs = self.pool_manager.list_server_configs().await;
        let fanout_timeout = self.fanout_timeout_secs;

        let mut handles = Vec::new();
        for (server_name, config, state) in server_configs {
            use scp_pool::lifecycle::ServerState;
            if !matches!(state, ServerState::Warm | ServerState::Hot) {
                continue;
            }
            let url = match config.url.clone() {
                Some(u) => u,
                None => {
                    warn!(
                        "Skipping stdio server {} in tools/list fan-out",
                        server_name
                    );
                    continue;
                }
            };
            let headers = config.headers.clone();
            let sn = server_name.clone();
            handles.push(tokio::spawn(async move {
                let mut transport = HttpServerTransport::new(url, headers);
                let req = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/list",
                    "params": {}
                });
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(fanout_timeout),
                    transport.send_request(&req),
                )
                .await;
                (sn, result)
            }));
        }

        let mut all_tools: Vec<Value> = extension_tools;
        let mut registry_updates: Vec<(String, Vec<ToolEntry>)> = Vec::new();

        for handle in handles {
            if let Ok((server_name, timeout_result)) = handle.await {
                match timeout_result {
                    Ok(Ok(response_body)) => {
                        let tools_array = response_body
                            .get("result")
                            .and_then(|r| r.get("tools"))
                            .or_else(|| response_body.get("tools"))
                            .and_then(|t| t.as_array())
                            .cloned()
                            .unwrap_or_default();

                        // Build ToolEntry list for registry
                        let entries: Vec<ToolEntry> = tools_array
                            .iter()
                            .filter_map(|tool| {
                                let name = tool.get("name")?.as_str()?.to_string();
                                Some(ToolEntry {
                                    original_name: name.clone(),
                                    qualified_name: format!("{}.{}", server_name, name),
                                    server_name: server_name.clone(),
                                    description: tool
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .map(|s| s.to_string()),
                                    input_schema: tool
                                        .get("inputSchema")
                                        .cloned()
                                        .unwrap_or(json!({})),
                                    tags: vec![],
                                    avg_response_tokens: 0.0,
                                    call_count: 0,
                                })
                            })
                            .collect();

                        registry_updates.push((server_name, entries));
                        all_tools.extend(tools_array);
                    }
                    Ok(Err(e)) => {
                        warn!("Backend {} tools/list error: {}", server_name, e)
                    }
                    Err(_) => warn!("Backend {} tools/list timed out", server_name),
                }
            }
        }

        // Update the tool registry with discovered tools
        {
            let mut registry = self.tool_registry.write().await;
            for (server_name, entries) in registry_updates {
                registry.rebuild_for_server(&server_name, entries);
            }
        }

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({ "tools": all_tools })),
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

        // Handle extension tools
        match tool_name.as_str() {
            "scp_info" => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": r#"{"name": "scp", "version": "0.2.0", "extensions": ["progressive_disclosure"]}"#
                            }
                        ]
                    })),
                    error: None,
                };
            }
            "scp_budget" => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": r#"{"remaining": 4000, "total": 4000}"#
                            }
                        ]
                    })),
                    error: None,
                };
            }
            "scp_budget_reset" => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": r#"{"status": "reset", "new_budget": 4000}"#
                            }
                        ]
                    })),
                    error: None,
                };
            }
            "scp_get_more" => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": r#"{"items": [], "offset": 0, "limit": 0}"#
                            }
                        ]
                    })),
                    error: None,
                };
            }
            _ => {}
        }

        // Lookup tool in registry
        let registry = self.tool_registry.read().await;
        match registry.lookup(&tool_name) {
            Some(tool_entry) => {
                let server_name = tool_entry.server_name.clone();
                drop(registry);

                match self
                    .call_backend(&server_name, "tools/call", request.params.clone())
                    .await
                {
                    Ok(response_body) => {
                        // Unwrap the JSON-RPC result wrapper if present
                        let result = response_body
                            .get("result")
                            .cloned()
                            .unwrap_or(response_body);
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: request.id.clone(),
                            result: Some(result),
                            error: None,
                        }
                    }
                    Err(e) => self.error_response(
                        request.id.clone(),
                        JsonRpcError::BACKEND_ERROR,
                        format!("Backend call failed: {}", e),
                    ),
                }
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

    /// Route a request to a specific backend server via HTTP.
    async fn call_backend(
        &self,
        server_name: &str,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, RouterError> {
        let config = self
            .pool_manager
            .get_server_config(server_name)
            .await
            .map_err(|_| RouterError::ServerNotFound(server_name.to_string()))?;

        let url = config.url.ok_or_else(|| {
            RouterError::PoolError(format!("Server {} has no HTTP URL", server_name))
        })?;

        let mut transport = HttpServerTransport::new(url, config.headers);

        let req_value = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params.unwrap_or(json!({}))
        });

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(self.fanout_timeout_secs),
            transport.send_request(&req_value),
        )
        .await
        .map_err(|_| {
            RouterError::PoolError(format!("Timeout calling backend {}", server_name))
        })?
        .map_err(|e| RouterError::PoolError(e.to_string()))?;

        Ok(response)
    }

    /// Handle initialize request (fan-out to all servers)
    async fn handle_initialize(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling initialize request");

        // Echo back the client's requested protocolVersion, defaulting to "2024-11-05"
        let protocol_version = request
            .params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05")
            .to_string();

        // For now, return basic capabilities (full implementation in P1.E.5)
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "protocolVersion": protocol_version,
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
