use crate::session_store::Session;
use scp_core::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_filter::dedup::DeliveryLog;
use scp_filter::pipeline::{FilterContext, FilterPipeline};
use scp_index::{ToolEntry, ToolRegistry};
use scp_pool::PoolManager;
use scp_transport::http_server::HttpServerTransport;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

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
    max_response_size_bytes: Option<usize>,
    filter_pipeline: Arc<FilterPipeline>,
}

impl Router {
    #[allow(dead_code)]
    /// Create a new router
    pub fn new(
        pool_manager: Arc<PoolManager>,
        tool_registry: Arc<RwLock<ToolRegistry>>,
        fanout_timeout_secs: u64,
        request_token_budget: usize,
        filter_pipeline: Arc<FilterPipeline>,
    ) -> Self {
        Self {
            pool_manager,
            tool_registry,
            fanout_timeout_secs,
            request_token_budget,
            max_response_size_bytes: Some(1_048_576),
            filter_pipeline,
        }
    }

    #[allow(dead_code)]
    /// Create a new router with a custom response size limit
    pub fn with_max_response_size(mut self, max_response_size_bytes: Option<usize>) -> Self {
        self.max_response_size_bytes = max_response_size_bytes;
        self
    }

    /// Perform an eager tools/list fanout to populate the registry.
    ///
    /// Returns `(tool_count, server_count)` where `server_count` is the number of
    /// backends that responded successfully. Servers that are not yet warm are
    /// skipped; errors from individual backends are logged but do not fail the call.
    pub async fn discover_tools(&self) -> (usize, usize) {
        let dummy_req = JsonRpcRequest::new(RequestId::Null, "tools/list".to_string(), None);
        let response = self.handle_tools_list(&dummy_req).await;

        // Count the tools and servers from the now-updated registry
        let registry = self.tool_registry.read().await;
        let tool_count = registry.tool_count();
        let server_count = registry.server_count();

        if let Some(result) = response.result {
            // Log at debug level in case callers want the raw response
            debug!("Eager tools/list fanout result: {}", result);
        }

        info!(
            "Eager tool discovery complete: {} tools registered from {} servers",
            tool_count, server_count
        );

        (tool_count, server_count)
    }

    /// Route a request to the appropriate backend.
    ///
    /// An optional `session` can be supplied; for `tools/call` requests the session's
    /// keyword accumulator is fed with the call arguments before the backend request is
    /// made, so relevance scoring has query terms ready.
    #[allow(dead_code)]
    pub async fn route(
        &self,
        request: JsonRpcRequest,
        session: Option<Arc<Mutex<Session>>>,
    ) -> JsonRpcResponse {
        match request.method.as_str() {
            "ping" => self.handle_ping(&request),
            "tools/list" => self.handle_tools_list(&request).await,
            "tools/call" => self.handle_tools_call(&request, session).await,
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
            json!({
                "name": "scp_search",
                "description": "Search available tools by keyword using TF-IDF scoring. Returns the most relevant tools for the given query, ranked by relevance score.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query to find relevant tools"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (default: 10)",
                            "default": 10
                        }
                    },
                    "required": ["query"]
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
                    let deadline = std::time::Duration::from_secs(fanout_timeout.min(5));
                    match tokio::time::timeout(deadline, async {
                        let pool = pool_manager_clone
                            .get_pool(&sn)
                            .await
                            .map_err(|e| e.to_string())?;
                        pool.call("tools/list", None)
                            .await
                            .map_err(|e| e.to_string())
                    })
                    .await
                    {
                        Ok(Ok(value)) => {
                            // Wrap in a pseudo-HTTP response envelope so the
                            // existing result-extraction logic works unchanged.
                            (sn, Ok(json!({ "result": value })))
                        }
                        Ok(Err(e)) => (sn, Err(e)),
                        Err(_) => (sn, Err("timeout".to_string())),
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
                                qualified_name: format!("{}/{}", server_name, name),
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

        // Build the tools list — always expose the qualified name (server_name/tool_name)
        // so clients can unambiguously route calls regardless of collision status.
        let mut all_tools: Vec<Value> = extension_tools;
        for (server_name, tools_array, _) in &backend_tools {
            for tool in tools_array {
                let Some(original_name) = tool.get("name").and_then(|n| n.as_str()) else {
                    continue;
                };
                let qualified_name = format!("{}/{}", server_name, original_name);

                let mut tool_obj = tool.clone();
                if let Some(obj) = tool_obj.as_object_mut() {
                    obj.insert("name".to_string(), json!(qualified_name));
                }
                all_tools.push(tool_obj);
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
    async fn handle_tools_call(
        &self,
        request: &JsonRpcRequest,
        session: Option<Arc<Mutex<Session>>>,
    ) -> JsonRpcResponse {
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

        // Feed tool call arguments into the keyword accumulator before reading state,
        // so top_k() includes terms from the current request.
        if let Some(sess) = &session {
            let args_opt = request
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned();
            if let Some(args) = args_opt {
                let mut s = sess.lock().unwrap_or_else(|e| e.into_inner());
                s.feed_tool_args(&args);
            }
        }

        // Extract session state (hold lock briefly to avoid holding across await)
        let (delivery_log, query_terms, budget, session_id_str) = if let Some(sess) = &session {
            let s = sess.lock().unwrap_or_else(|e| e.into_inner());
            let terms = s.current_query_terms(20);
            let budget = s.token_budget_remaining;
            let sid = s.id.clone();
            let log = s.delivery_log.clone();
            (log, terms, budget, sid)
        } else {
            (
                Arc::new(Mutex::new(DeliveryLog::new(1000))),
                vec![],
                self.request_token_budget,
                "anonymous".to_string(),
            )
        };

        // If tool_name is slash-qualified (e.g. "memory-global/search_memory"), route directly
        // to the named server without a registry lookup.
        // Also accept legacy dot-qualified form (e.g. "memory-global.search_memory") for
        // backwards compatibility.
        let slash_pos = tool_name.find('/').or_else(|| tool_name.find('.'));
        if let Some(sep_pos) = slash_pos {
            let server_name = tool_name[..sep_pos].to_string();
            let actual_tool = tool_name[sep_pos + 1..].to_string();

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

                    // Run filter pipeline on backend response
                    let content_value = result
                        .get("content")
                        .cloned()
                        .unwrap_or_else(|| result.clone());
                    let request_id = Uuid::new_v4().to_string();
                    let filter_ctx = FilterContext {
                        session_id: session_id_str.clone(),
                        tool_name: actual_tool.clone(),
                        budget_tokens: budget,
                        query_terms: query_terms.clone(),
                        delivery_log: delivery_log.clone(),
                        short_circuit_below_tokens: self.filter_pipeline.short_circuit_below_tokens,
                        request_id: request_id.clone(),
                    };
                    let filter_result = self.filter_pipeline.run(&content_value, &filter_ctx).await;

                    if !filter_result.dropped_chunks.is_empty() {
                        info!(
                            session_id = %session_id_str,
                            tool_name = %actual_tool,
                            tokens_delivered = filter_result.tokens_delivered,
                            dropped_count = filter_result.dropped_chunks.len(),
                            "filter pipeline dropped chunks"
                        );
                    }

                    // Update session state
                    if let Some(sess) = &session {
                        let mut s = sess.lock().unwrap_or_else(|e| e.into_inner());
                        s.token_budget_remaining = s
                            .token_budget_remaining
                            .saturating_sub(filter_result.tokens_delivered);
                        if !filter_result.dropped_chunks.is_empty() {
                            s.store_chunks(
                                request_id.clone(),
                                filter_result.dropped_chunks.clone(),
                            );
                        }
                    }

                    let filtered_result = json!({
                        "content": [{"type": "text", "text": filter_result.content}]
                    });
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.clone(),
                        result: Some(filtered_result),
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
                let (remaining, total) = if let Some(sess) = &session {
                    let s = sess.lock().unwrap_or_else(|e| e.into_inner());
                    (s.token_budget_remaining, self.request_token_budget)
                } else {
                    (self.request_token_budget, self.request_token_budget)
                };
                let text = serde_json::to_string(&json!({
                    "remaining": remaining,
                    "total": total,
                    "used": total.saturating_sub(remaining)
                }))
                .unwrap_or_default();
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [{"type": "text", "text": text}]
                    })),
                    error: None,
                };
            }
            "scp_budget_reset" => {
                if let Some(sess) = &session {
                    let mut s = sess.lock().unwrap_or_else(|e| e.into_inner());
                    s.token_budget_remaining = self.request_token_budget;
                }
                let text = serde_json::to_string(&json!({
                    "status": "reset",
                    "new_budget": self.request_token_budget
                }))
                .unwrap_or_default();
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [{"type": "text", "text": text}]
                    })),
                    error: None,
                };
            }
            "scp_get_more" => {
                let args = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("arguments"))
                    .or(request.params.as_ref());
                let request_id = args
                    .and_then(|p| p.get("request_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let offset = args
                    .and_then(|p| p.get("offset"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let limit = args
                    .and_then(|p| p.get("limit"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;

                let (items, total) = if let Some(sess) = &session {
                    let s = sess.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(chunks) = s.get_chunks(&request_id) {
                        let total = chunks.len();
                        let slice: Vec<String> = chunks
                            .iter()
                            .skip(offset)
                            .take(limit)
                            .map(|c| c.text.clone())
                            .collect();
                        (slice, total)
                    } else {
                        (vec![], 0)
                    }
                } else {
                    (vec![], 0)
                };

                let text = serde_json::to_string(&json!({
                    "items": items,
                    "offset": offset,
                    "limit": limit,
                    "total": total,
                    "has_more": offset + limit < total
                }))
                .unwrap_or_default();
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [{"type": "text", "text": text}]
                    })),
                    error: None,
                };
            }
            "scp_search" => {
                let query = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("query"))
                    .and_then(|q| q.as_str())
                    .unwrap_or("")
                    .to_string();
                let limit = request
                    .params
                    .as_ref()
                    .and_then(|p| p.get("limit"))
                    .and_then(|l| l.as_u64())
                    .unwrap_or(10) as usize;

                let registry = self.tool_registry.read().await;
                let results = registry.search_tools(&query);
                let results: Vec<Value> = results
                    .into_iter()
                    .take(limit)
                    .map(|(score, entry)| {
                        json!({
                            "name": entry.qualified_name,
                            "score": score,
                            "description": entry.description,
                        })
                    })
                    .collect();

                let text = serde_json::to_string(&json!({
                    "query": query,
                    "results": results,
                }))
                .unwrap_or_default();

                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [{"type": "text", "text": text}]
                    })),
                    error: None,
                };
            }
            _ => {}
        }

        // If the registry is empty, trigger a tools/list to populate it before
        // attempting the lookup. This handles the cold-start case where the client
        // calls tools/call before tools/list has ever been called.
        {
            let registry = self.tool_registry.read().await;
            if registry.tool_count() == 0 {
                drop(registry);
                let list_req = JsonRpcRequest::new(RequestId::Null, "tools/list".to_string(), None);
                let _ = self.handle_tools_list(&list_req).await;
            }
        }

        // Lookup tool in registry
        let registry = self.tool_registry.read().await;
        match registry.lookup(&tool_name) {
            Some(tool_entry) => {
                let server_name = tool_entry.server_name.clone();
                let original_name = tool_entry.original_name.clone();
                drop(registry);

                // Rewrite the "name" field to the bare original_name before forwarding.
                // The client may have sent a qualified name (e.g. "memory-global/search_memory")
                // or a bare name (e.g. "search_memory"); the backend always expects the bare form.
                let mut forwarded_params = request.params.clone().unwrap_or(json!({}));
                if let Some(obj) = forwarded_params.as_object_mut() {
                    obj.insert("name".to_string(), json!(original_name));
                }

                match self
                    .call_backend(&server_name, "tools/call", Some(forwarded_params))
                    .await
                {
                    Ok(response_body) => {
                        // Unwrap the JSON-RPC result wrapper if present
                        let result = response_body
                            .get("result")
                            .cloned()
                            .unwrap_or(response_body);

                        // Run filter pipeline on backend response
                        let content_value = result
                            .get("content")
                            .cloned()
                            .unwrap_or_else(|| result.clone());
                        let request_id = Uuid::new_v4().to_string();
                        let filter_ctx = FilterContext {
                            session_id: session_id_str.clone(),
                            tool_name: original_name.clone(),
                            budget_tokens: budget,
                            query_terms: query_terms.clone(),
                            delivery_log: delivery_log.clone(),
                            short_circuit_below_tokens: self
                                .filter_pipeline
                                .short_circuit_below_tokens,
                            request_id: request_id.clone(),
                        };
                        let filter_result =
                            self.filter_pipeline.run(&content_value, &filter_ctx).await;

                        if !filter_result.dropped_chunks.is_empty() {
                            info!(
                                session_id = %session_id_str,
                                tool_name = %original_name,
                                tokens_delivered = filter_result.tokens_delivered,
                                dropped_count = filter_result.dropped_chunks.len(),
                                "filter pipeline dropped chunks"
                            );
                        }

                        // Update session state
                        if let Some(sess) = &session {
                            let mut s = sess.lock().unwrap_or_else(|e| e.into_inner());
                            s.token_budget_remaining = s
                                .token_budget_remaining
                                .saturating_sub(filter_result.tokens_delivered);
                            if !filter_result.dropped_chunks.is_empty() {
                                s.store_chunks(
                                    request_id.clone(),
                                    filter_result.dropped_chunks.clone(),
                                );
                            }
                        }

                        let filtered_result = json!({
                            "content": [{"type": "text", "text": filter_result.content}]
                        });
                        JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: request.id.clone(),
                            result: Some(filtered_result),
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
                let total = registry.tool_count();
                let sample: Vec<String> = registry
                    .all_tools()
                    .iter()
                    .take(10)
                    .map(|e| e.qualified_name.clone())
                    .collect();
                drop(registry);
                let hint = if total == 0 {
                    "registry is empty — call tools/list first to discover available tools"
                        .to_string()
                } else {
                    format!(
                        "{} tool{} registered: {}",
                        total,
                        if total == 1 { "" } else { "s" },
                        sample.join(", ")
                    )
                };
                self.error_response(
                    request.id.clone(),
                    JsonRpcError::INVALID_PARAMS,
                    format!("Tool not found: {} ({})", tool_name, hint),
                )
            }
        }
    }

    /// Truncate the `content[0].text` field of a JSON-RPC result if the serialized
    /// response exceeds `limit` bytes. Appends a notice with the actual and limit sizes.
    fn maybe_truncate_response(&self, mut response: Value) -> Value {
        let limit = match self.max_response_size_bytes {
            Some(l) => l,
            None => return response,
        };

        let serialized_len = response.to_string().len();
        if serialized_len <= limit {
            return response;
        }

        // Navigate to result.content[0].text and truncate it
        if let Some(result) = response.get_mut("result") {
            if let Some(content) = result.get_mut("content") {
                if let Some(first) = content.get_mut(0) {
                    if let Some(text) = first.get_mut("text") {
                        if let Some(s) = text.as_str() {
                            let notice = format!(
                                "\n\n[Response truncated: {} bytes exceeded {} bytes limit]",
                                serialized_len, limit
                            );
                            // Keep as many bytes of the original text as we can fit
                            let keep = limit.saturating_sub(notice.len());
                            let truncated = if keep < s.len() {
                                let mut t = s[..keep].to_string();
                                t.push_str(&notice);
                                t
                            } else {
                                let mut t = s.to_string();
                                t.push_str(&notice);
                                t
                            };
                            *text = json!(truncated);
                        }
                    }
                }
            }
        }

        response
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

            Ok(self.maybe_truncate_response(response))
        } else if config.command.is_some() {
            // stdio backend — use shared pool; wrap get_pool + call in a single timeout
            // so initialization latency (ensure_initialized 30s window) cannot exceed
            // the configured fanout limit.
            let pool_manager = self.pool_manager.clone();
            let sn = server_name.to_string();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(self.fanout_timeout_secs),
                async move {
                    let pool = pool_manager
                        .get_pool(&sn)
                        .await
                        .map_err(|e| RouterError::PoolError(e.to_string()))?;
                    pool.call(method, params)
                        .await
                        .map_err(|e| RouterError::PoolError(e.to_string()))
                },
            )
            .await
            .map_err(|_| {
                RouterError::PoolError(format!("Timeout calling backend {}", server_name))
            })??;

            // Wrap result to match HTTP response shape expected by callers
            Ok(self.maybe_truncate_response(json!({ "result": result })))
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
    use scp_filter::pipeline::FilterPipeline;

    fn make_filter_pipeline() -> Arc<FilterPipeline> {
        Arc::new(FilterPipeline::new(
            &scp_core::config::FilterConfig::default(),
        ))
    }

    #[tokio::test]
    async fn test_router_ping() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "ping".to_string(),
            params: None,
        };

        let response = router.route(request, None).await;
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_router_unknown_method() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = router.route(request, None).await;
        assert!(response.error.is_some());
    }

    #[tokio::test]
    async fn test_tool_not_found_empty_registry_hint() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({ "name": "nonexistent_tool", "arguments": {} })),
        };

        let response = router.route(request, None).await;
        assert!(response.error.is_some());
        let msg = response.error.unwrap().message;
        assert!(
            msg.contains("registry is empty"),
            "Expected empty-registry hint in: {msg}"
        );
    }

    #[tokio::test]
    async fn test_tool_not_found_with_registered_tools_hint() {
        use scp_index::ToolEntry;

        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

        // Pre-populate the registry with a known tool so the error includes the count
        {
            let mut registry = tool_registry.write().await;
            registry.register_tools(
                "test-server",
                vec![ToolEntry {
                    original_name: "known_tool".to_string(),
                    qualified_name: "test-server/known_tool".to_string(),
                    server_name: "test-server".to_string(),
                    description: None,
                    input_schema: serde_json::json!({}),
                    tags: vec![],
                    avg_response_tokens: 0.0,
                    call_count: 0,
                }],
            );
        }

        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({ "name": "unknown_tool", "arguments": {} })),
        };

        let response = router.route(request, None).await;
        assert!(response.error.is_some());
        let msg = response.error.unwrap().message;
        assert!(
            msg.contains("1 tool registered"),
            "Expected tool count hint in: {msg}"
        );
        assert!(
            msg.contains("known_tool"),
            "Expected sample tool name in: {msg}"
        );
    }

    #[test]
    fn test_response_truncated_when_exceeds_limit() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline())
            .with_max_response_size(Some(100));

        // Build a response whose JSON serialization clearly exceeds 100 bytes
        let large_text = "x".repeat(200);
        let response = serde_json::json!({
            "result": {
                "content": [{"type": "text", "text": large_text}]
            }
        });

        let truncated = router.maybe_truncate_response(response);
        let text = truncated["result"]["content"][0]["text"]
            .as_str()
            .expect("text field missing");

        assert!(
            text.contains("[Response truncated:"),
            "Expected truncation notice, got: {text}"
        );
        assert!(
            truncated.to_string().len() < 200 + 300,
            "Response should be significantly smaller than original"
        );
    }

    #[test]
    fn test_response_not_truncated_when_under_limit() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let router = Router::new(pool_manager, tool_registry, 5, 4000, make_filter_pipeline())
            .with_max_response_size(Some(1_048_576));

        let small_text = "hello world";
        let response = serde_json::json!({
            "result": {
                "content": [{"type": "text", "text": small_text}]
            }
        });

        let result = router.maybe_truncate_response(response);
        let text = result["result"]["content"][0]["text"]
            .as_str()
            .expect("text field missing");

        assert_eq!(text, small_text, "Small response must not be truncated");
        assert!(
            !text.contains("[Response truncated:"),
            "No truncation notice expected"
        );
    }
}
