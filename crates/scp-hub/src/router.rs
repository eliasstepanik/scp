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
    /// The requested tool name is not registered in the tool registry.
    ToolNotFound(String),

    #[error("Server not found: {0}")]
    /// The target backend server is not known to the pool manager.
    ServerNotFound(String),

    #[error("Pool error: {0}")]
    /// An error occurred when communicating with the connection pool or backend.
    PoolError(String),

    #[error("Invalid request: {0}")]
    /// The incoming JSON-RPC request is malformed or missing required fields.
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

        // Each handle resolves to (server_name, Result<response_body, error_string>)
        let mut handles: Vec<tokio::task::JoinHandle<(String, Result<Value, String>)>> = Vec::new();
        for (server_name, config, state) in server_configs {
            use scp_pool::lifecycle::ServerState;
            if !matches!(state, ServerState::Warm | ServerState::Hot) {
                continue;
            }

            if let Some(url) = config.url.clone() {
                // HTTP backend
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
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(fanout_timeout.min(5)),
                        transport.send_request(&req),
                    )
                    .await
                    {
                        Ok(Ok(body)) => (sn, Ok(body)),
                        Ok(Err(e)) => (sn, Err(e.to_string())),
                        Err(_) => (sn, Err("timeout".to_string())),
                    }
                }));
            } else if config.command.is_some() {
                // stdio backend — use shared pool
                let pool_manager_clone = self.pool_manager.clone();
                let sn = server_name.clone();
                handles.push(tokio::spawn(async move {
                    match pool_manager_clone.get_pool(&sn).await {
                        Ok(pool) => {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(fanout_timeout.min(5)),
                                pool.call("tools/list", None),
                            )
                            .await
                            {
                                Ok(Ok(value)) => {
                                    // Wrap in a pseudo-HTTP response envelope so the
                                    // existing result-extraction logic works unchanged.
                                    (sn, Ok(json!({ "result": value })))
                                }
                                Ok(Err(e)) => (sn, Err(e.to_string())),
                                Err(_) => (sn, Err("timeout".to_string())),
                            }
                        }
                        Err(e) => (sn, Err(e.to_string())),
                    }
                }));
            } else {
                warn!("Skipping server {} — no url and no command", server_name);
            }
        }

        // Collect (server_name, raw_tools_array, entries) before updating registry
        let mut backend_tools: Vec<(String, Vec<Value>, Vec<ToolEntry>)> = Vec::new();

        let results = futures::future::join_all(handles).await;
        for (server_name, result) in results.into_iter().flatten() {
            match result {
                Ok(response_body) => {
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
                                input_schema: tool.get("inputSchema").cloned().unwrap_or(json!({})),
                                tags: vec![],
                                avg_response_tokens: 0.0,
                                call_count: 0,
                            })
                        })
                        .collect();

                    backend_tools.push((server_name, tools_array, entries));
                }
                Err(e) => {
                    warn!("Backend {} tools/list error: {}", server_name, e)
                }
            }
        }

        // Update the tool registry with discovered tools
        {
            let mut registry = self.tool_registry.write().await;
            for (server_name, _, entries) in &backend_tools {
                registry.rebuild_for_server(server_name, entries.clone());
            }
        }

        // Build the tools list using registry-aware names:
        // - If a tool has no collision, the alias exists → use original (unqualified) name
        // - If a tool has a collision, no alias exists → use qualified name so callers can route it
        let mut all_tools: Vec<Value> = extension_tools;
        {
            let registry = self.tool_registry.read().await;
            for (server_name, tools_array, _) in &backend_tools {
                for tool in tools_array {
                    let Some(original_name) = tool.get("name").and_then(|n| n.as_str()) else {
                        continue;
                    };
                    let qualified_name = format!("{}.{}", server_name, original_name);

                    // Determine which name to expose: prefer the unqualified alias when it
                    // resolves unambiguously to this server's tool; fall back to qualified.
                    let exposed_name = match registry.lookup(original_name) {
                        Some(entry) if entry.server_name == *server_name => {
                            original_name.to_string()
                        }
                        _ => qualified_name,
                    };

                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(exposed_name));
                    }
                    all_tools.push(tool_obj);
                }
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

        // If tool_name is dot-qualified (e.g. "memory-mcp.list_memories"), route directly
        // to the named server without a registry lookup.
        if let Some(dot_pos) = tool_name.find('.') {
            let server_name = tool_name[..dot_pos].to_string();
            let actual_tool = tool_name[dot_pos + 1..].to_string();

            // Rewrite the "name" field in params to the unqualified tool name
            let mut new_params = request.params.clone().unwrap_or(serde_json::json!({}));
            if let Some(obj) = new_params.as_object_mut() {
                obj.insert("name".to_string(), serde_json::json!(actual_tool));
            }

            return match self
                .call_backend(&server_name, "tools/call", Some(new_params))
                .await
            {
                Ok(response_body) => {
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
                    format!("Backend error: {}", e),
                ),
            };
        }

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

    /// Route a request to a specific backend server (HTTP or stdio).
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

        if let Some(url) = config.url {
            // HTTP backend
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
        } else if config.command.is_some() {
            // stdio backend — use shared pool
            let pool = self
                .pool_manager
                .get_pool(server_name)
                .await
                .map_err(|e| RouterError::PoolError(e.to_string()))?;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(self.fanout_timeout_secs),
                pool.call(method, params),
            )
            .await
            .map_err(|_| {
                RouterError::PoolError(format!("Timeout calling backend {}", server_name))
            })?
            .map_err(|e| RouterError::PoolError(e.to_string()))?;

            // Wrap result to match HTTP response shape expected by callers
            Ok(json!({ "result": result }))
        } else {
            Err(RouterError::PoolError(format!(
                "Server {} has no url and no command",
                server_name
            )))
        }
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
