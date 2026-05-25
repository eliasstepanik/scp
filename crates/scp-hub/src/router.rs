use crate::session_store::Session;
use scp_core::config::ExposureConfig;
use scp_core::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, RequestId};
use scp_filter::dedup::DeliveryLog;
use scp_filter::pipeline::{FilterContext, FilterPipeline};
use scp_index::{ToolEntry, ToolRegistry};
use scp_pool::PoolManager;
use scp_transport::http_server::HttpServerTransport;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
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
    exposure: ExposureConfig,
    always_include: Vec<String>,
    max_tools_exposed: usize,
    /// Sender used to subscribe to the shutdown signal inside `call_backend`.
    /// When cancelled, in-flight backend calls are aborted immediately.
    shutdown_tx: broadcast::Sender<()>,
}

impl Router {
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    /// Create a new router
    pub fn new(
        pool_manager: Arc<PoolManager>,
        tool_registry: Arc<RwLock<ToolRegistry>>,
        fanout_timeout_secs: u64,
        request_token_budget: usize,
        filter_pipeline: Arc<FilterPipeline>,
        exposure: ExposureConfig,
        always_include: Vec<String>,
        max_tools_exposed: usize,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self {
            pool_manager,
            tool_registry,
            fanout_timeout_secs,
            request_token_budget,
            // Defaults to None (no cap). Callers should set this to ≤1MB (e.g. via
            // `with_max_response_size`) for exec/shell backends to prevent memory spikes
            // from extremely large tool responses before the filter pipeline runs.
            max_response_size_bytes: None,
            filter_pipeline,
            exposure,
            always_include,
            max_tools_exposed,
            shutdown_tx,
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

        // NOTE: handle_tools_list applies the exposure filter to its return value,
        // so the response tool count reflects only pinned/exposed tools.
        // The full tool count is available from the ToolRegistry directly.
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

        // Build a map of server_name → display prefix (name_prefix or server_name itself)
        let prefix_map: std::collections::HashMap<String, String> = server_configs
            .iter()
            .map(|(sn, cfg, _)| {
                let prefix = cfg.name_prefix.clone().unwrap_or_else(|| sn.clone());
                (sn.clone(), prefix)
            })
            .collect();

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
                let raw_url = config.raw_url;
                let connect_timeout = config.timeouts.connect_secs;
                let request_timeout = config.timeouts.request_secs;
                let sn = server_name.clone();
                handles.push(tokio::spawn(async move {
                    let mut transport = HttpServerTransport::new(
                        url,
                        headers,
                        raw_url,
                        connect_timeout,
                        request_timeout,
                    );
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

        // Collect (server_name, display_prefix, raw_tools_array, entries) before updating registry
        let mut backend_tools: Vec<(String, String, Vec<Value>, Vec<ToolEntry>)> = Vec::new();

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

                    // Resolve display prefix for this server (may differ from server_name)
                    let display_prefix = prefix_map
                        .get(&server_name)
                        .cloned()
                        .unwrap_or_else(|| server_name.clone());

                    // Build ToolEntry list for registry.
                    // qualified_name uses the real server_name (canonical internal key).
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

                    backend_tools.push((server_name, display_prefix, tools_array, entries));
                }
                Err(e) => {
                    warn!("Backend {} tools/list error: {}", server_name, e)
                }
            }
        }

        // Update the tool registry with discovered tools and register display aliases
        {
            let mut registry = self.tool_registry.write().await;
            for (server_name, display_prefix, _, entries) in &backend_tools {
                registry.rebuild_for_server(server_name, display_prefix, entries.clone());
                // If the server has a custom name_prefix, register display aliases so that
                // tools/call with the prefixed name resolves to the right backend.
                registry.register_display_aliases(server_name, display_prefix);
            }
        }

        // Build the outbound tools/list with exposure filter.
        // Priority order (backend tools only — extension tools are always first and uncapped):
        //   1. always_include tools (matched by qualified "server/tool" or "prefix/tool" name)
        //   2. pinned_servers tools (in server-list order)
        //   3. Stop at max_tools_exposed total backend tools
        //
        // The exposed JSON tool name uses the display_prefix (= name_prefix or server_name).
        let cap = self.max_tools_exposed;
        let always_include_set: std::collections::HashSet<&str> =
            self.always_include.iter().map(|s| s.as_str()).collect();
        let pinned_server_set: std::collections::HashSet<&str> = self
            .exposure
            .pinned_servers
            .iter()
            .map(|s| s.as_str())
            .collect();

        let mut exposed_backend_tools: Vec<Value> = Vec::new();

        // Pass 1: always_include tools (by canonical or display qualified name match)
        'outer_always: for (server_name, display_prefix, tools_array, _) in &backend_tools {
            for tool in tools_array {
                if exposed_backend_tools.len() >= cap {
                    break 'outer_always;
                }
                let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let canonical_qname = format!("{}/{}", server_name, original_name);
                let display_qname = format!("{}/{}", display_prefix, original_name);
                if always_include_set.contains(canonical_qname.as_str())
                    || always_include_set.contains(display_qname.as_str())
                {
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(display_qname));
                    }
                    exposed_backend_tools.push(tool_obj);
                }
            }
        }

        // Pass 2: pinned server tools (skip any already added in pass 1)
        let already_added: std::collections::HashSet<String> = exposed_backend_tools
            .iter()
            .filter_map(|t| {
                t.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        'outer_pinned: for server_name in &self.exposure.pinned_servers {
            if exposed_backend_tools.len() >= cap {
                break 'outer_pinned;
            }
            for (sn, display_prefix, tools_array, _) in &backend_tools {
                if sn != server_name {
                    continue;
                }
                for tool in tools_array {
                    if exposed_backend_tools.len() >= cap {
                        break;
                    }
                    let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let display_qname = format!("{}/{}", display_prefix, original_name);
                    if already_added.contains(&display_qname) {
                        continue;
                    }
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(display_qname));
                    }
                    exposed_backend_tools.push(tool_obj);
                }
            }
        }

        // If cap not yet reached and pinned_servers is empty (open mode), include all remaining
        // tools so that the default zero-config behavior is unchanged.
        if pinned_server_set.is_empty() && always_include_set.is_empty() {
            let already_added_open: std::collections::HashSet<String> = exposed_backend_tools
                .iter()
                .filter_map(|t| {
                    t.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            'outer_open: for (_, display_prefix, tools_array, _) in &backend_tools {
                for tool in tools_array {
                    if exposed_backend_tools.len() >= cap {
                        break 'outer_open;
                    }
                    let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let display_qname = format!("{}/{}", display_prefix, original_name);
                    if already_added_open.contains(&display_qname) {
                        continue;
                    }
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(display_qname));
                    }
                    exposed_backend_tools.push(tool_obj);
                }
            }
        }

        // Build final tools list: SCP extension tools (uncapped) + filtered backend tools
        let mut all_tools: Vec<Value> = extension_tools;
        all_tools.extend(exposed_backend_tools);

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({ "tools": all_tools })),
            error: None,
        }
    }

    /// Pure filtering helper used by tests.
    ///
    /// Given a list of `(server_name, tools_array)` pairs (the same data that
    /// `handle_tools_list` collects from backends), applies the exposure filter
    /// (`pinned_servers`, `always_include`, `max_tools_exposed`) and returns the
    /// slice of backend tools that should appear in the outbound `tools/list`.
    ///
    /// Extension tools are **not** included — this is backend-tools only.
    #[cfg(test)]
    pub(crate) fn apply_exposure_filter(
        &self,
        backend_tools: &[(String, Vec<Value>)],
    ) -> Vec<Value> {
        let cap = self.max_tools_exposed;
        let always_include_set: std::collections::HashSet<&str> =
            self.always_include.iter().map(|s| s.as_str()).collect();
        let pinned_server_set: std::collections::HashSet<&str> = self
            .exposure
            .pinned_servers
            .iter()
            .map(|s| s.as_str())
            .collect();

        let mut exposed: Vec<Value> = Vec::new();

        // Pass 1: always_include tools
        'outer_always: for (server_name, tools_array) in backend_tools {
            for tool in tools_array {
                if exposed.len() >= cap {
                    break 'outer_always;
                }
                let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let qualified = format!("{}/{}", server_name, original_name);
                if always_include_set.contains(qualified.as_str()) {
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(qualified));
                    }
                    exposed.push(tool_obj);
                }
            }
        }

        // Pass 2: pinned server tools
        let already_added: std::collections::HashSet<String> = exposed
            .iter()
            .filter_map(|t| {
                t.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        'outer_pinned: for server_name in &self.exposure.pinned_servers {
            if exposed.len() >= cap {
                break 'outer_pinned;
            }
            for (sn, tools_array) in backend_tools {
                if sn != server_name {
                    continue;
                }
                for tool in tools_array {
                    if exposed.len() >= cap {
                        break;
                    }
                    let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let qualified = format!("{}/{}", sn, original_name);
                    if already_added.contains(&qualified) {
                        continue;
                    }
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(qualified));
                    }
                    exposed.push(tool_obj);
                }
            }
        }

        // Open mode: if no pinned_servers and no always_include, include all up to cap
        if pinned_server_set.is_empty() && always_include_set.is_empty() {
            let already_added_open: std::collections::HashSet<String> = exposed
                .iter()
                .filter_map(|t| {
                    t.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            'outer_open: for (server_name, tools_array) in backend_tools {
                for tool in tools_array {
                    if exposed.len() >= cap {
                        break 'outer_open;
                    }
                    let original_name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let qualified = format!("{}/{}", server_name, original_name);
                    if already_added_open.contains(&qualified) {
                        continue;
                    }
                    let mut tool_obj = tool.clone();
                    if let Some(obj) = tool_obj.as_object_mut() {
                        obj.insert("name".to_string(), json!(qualified));
                    }
                    exposed.push(tool_obj);
                }
            }
        }

        exposed
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
        //
        // When name_prefix is configured (e.g. "proxy" for server "ssh-proxy"), the client
        // sends "proxy/exec". We first check the registry for a display-alias that maps the
        // prefixed name to the real server; if found, route to the real server. Otherwise fall
        // back to using the prefix as the server name directly (the legacy behavior).
        let slash_pos = tool_name.find('/').or_else(|| tool_name.find('.'));
        if let Some(sep_pos) = slash_pos {
            // Try registry lookup first to resolve potential name_prefix aliases
            let (server_name, actual_tool) = {
                let registry = self.tool_registry.read().await;
                if let Some(entry) = registry.lookup(&tool_name) {
                    let sn = entry.server_name.clone();
                    let ot = entry.original_name.clone();
                    drop(registry);
                    (sn, ot)
                } else {
                    drop(registry);
                    (
                        tool_name[..sep_pos].to_string(),
                        tool_name[sep_pos + 1..].to_string(),
                    )
                }
            };

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
                                filter_result.dropped_chunks,
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
                // Safety: PoolError::Cancelled is returned when the backend stdio process crashes
                // mid-request. This arm ensures the crash is converted to a JSON-RPC error and
                // never propagates as a Rust panic to the client session.
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
                let registry = self.tool_registry.read().await;

                // Build server summary: {name -> tool_count}
                let mut server_counts: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for tool in registry.all_tools() {
                    if let Some(slash) = tool.qualified_name.find('/') {
                        let server = &tool.qualified_name[..slash];
                        *server_counts.entry(server.to_string()).or_insert(0) += 1;
                    }
                }
                drop(registry);

                // Sort servers by name for deterministic output
                let mut servers: Vec<Value> = server_counts
                    .into_iter()
                    .map(|(name, tool_count)| json!({"name": name, "tool_count": tool_count}))
                    .collect();
                servers.sort_by(|a, b| {
                    a.get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .cmp(b.get("name").and_then(|n| n.as_str()).unwrap_or(""))
                });

                let total_tools: usize = servers
                    .iter()
                    .filter_map(|s| s.get("tool_count").and_then(|c| c.as_u64()))
                    .sum::<u64>() as usize;

                let info = json!({
                    "name": "scp",
                    "version": "0.2.0",
                    "extensions": ["progressive_disclosure"],
                    "total_tools": total_tools,
                    "exposed_tools": self.max_tools_exposed,
                    "servers": servers,
                    "pinned_servers": self.exposure.pinned_servers,
                    "hint": "Use scp_search(query) to discover tools. Call any tool directly as 'server/tool_name' even if not in tools/list."
                });

                let text = serde_json::to_string(&info).unwrap_or_else(|_| "{}".to_string());

                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!({
                        "content": [{"type": "text", "text": text}]
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
                let args = request.params.as_ref().and_then(|p| p.get("arguments"));
                let query = args
                    .and_then(|a| a.get("query"))
                    .and_then(|q| q.as_str())
                    .unwrap_or("")
                    .to_string();
                let limit = args
                    .and_then(|a| a.get("limit"))
                    .and_then(|l| l.as_u64())
                    .unwrap_or(10) as usize;

                let registry = self.tool_registry.read().await;

                // Primary: TF-IDF search over name + qualified_name + description
                let results = registry.search_tools_scored(&query);

                // Fallback: if no scored hits, do substring matching
                let results = if results.is_empty() {
                    let query_lower = query.to_lowercase();
                    let mut fallback: Vec<(f32, ToolEntry)> = registry
                        .all_tools()
                        .into_iter()
                        .filter(|e| {
                            e.qualified_name.to_lowercase().contains(&query_lower)
                                || e.description
                                    .as_deref()
                                    .unwrap_or("")
                                    .to_lowercase()
                                    .contains(&query_lower)
                        })
                        .map(|e| (0.1_f32, e.clone()))
                        .collect();
                    fallback.truncate(limit);
                    fallback
                } else {
                    results
                };
                drop(registry);

                let search_returns_schema = self.exposure.search_returns_schema;
                let results: Vec<Value> = results
                    .into_iter()
                    .take(limit)
                    .map(|(score, entry)| {
                        let mut obj = json!({
                            "name": entry.qualified_name,
                            "score": score,
                            "description": entry.description,
                        });
                        if search_returns_schema {
                            // input_schema is always a Value; include unless it's null
                            if !entry.input_schema.is_null() {
                                obj.as_object_mut()
                                    .unwrap()
                                    .insert("inputSchema".to_string(), entry.input_schema.clone());
                            }
                        }
                        obj
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
                                s.store_chunks(request_id.clone(), filter_result.dropped_chunks);
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

        // Subscribe to the shutdown signal so we can abort in-flight calls immediately.
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        if let Some(url) = config.url {
            // HTTP backend
            let mut transport = HttpServerTransport::new(
                url,
                config.headers,
                config.raw_url,
                config.timeouts.connect_secs,
                config.timeouts.request_secs,
            );

            let req_value = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params.unwrap_or(json!({}))
            });

            let backend_fut = tokio::time::timeout(
                std::time::Duration::from_secs(self.fanout_timeout_secs),
                transport.send_request(&req_value),
            );

            let response = tokio::select! {
                res = backend_fut => {
                    res.map_err(|_| {
                        RouterError::PoolError(format!("Timeout calling backend {}", server_name))
                    })?
                    .map_err(|e| RouterError::PoolError(e.to_string()))?
                }
                _ = shutdown_rx.recv() => {
                    return Err(RouterError::PoolError(format!(
                        "Shutdown: aborted in-flight call to backend {}", server_name
                    )));
                }
            };

            Ok(self.maybe_truncate_response(response))
        } else if config.command.is_some() {
            // stdio backend — use shared pool; wrap get_pool + call in a single timeout
            // so initialization latency (ensure_initialized 30s window) cannot exceed
            // the configured fanout limit.
            let pool_manager = self.pool_manager.clone();
            let sn = server_name.to_string();
            let backend_fut = tokio::time::timeout(
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
            );

            let result = tokio::select! {
                res = backend_fut => {
                    res.map_err(|_| {
                        RouterError::PoolError(format!("Timeout calling backend {}", server_name))
                    })??
                }
                _ = shutdown_rx.recv() => {
                    return Err(RouterError::PoolError(format!(
                        "Shutdown: aborted in-flight call to backend {}", server_name
                    )));
                }
            };

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
        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        );

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
        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        );

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
        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        );

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

        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        );

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
        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        )
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
        let (shutdown_tx, _) = broadcast::channel(1);
        let router = Router::new(
            pool_manager,
            tool_registry,
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig::default(),
            vec![],
            50,
            shutdown_tx,
        )
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

    // =========================================================================
    // TE6: Tool Exposure filter unit tests
    // =========================================================================

    /// Build a fake `(server_name, tools_array)` pair with `n` numbered tools.
    fn fake_server_tools(server: &str, n: usize) -> (String, Vec<Value>) {
        let tools: Vec<Value> = (1..=n)
            .map(|i| {
                json!({
                    "name": format!("tool_{}", i),
                    "description": format!("Tool {} on {}", i, server),
                    "inputSchema": {"type": "object", "properties": {}}
                })
            })
            .collect();
        (server.to_string(), tools)
    }

    /// Build a Router configured for exposure filtering tests.
    fn make_exposure_router(
        pinned_servers: Vec<&str>,
        always_include: Vec<&str>,
        max_tools_exposed: usize,
    ) -> Router {
        let exposure = scp_core::config::ExposureConfig {
            pinned_servers: pinned_servers.into_iter().map(|s| s.to_string()).collect(),
            allow_unlisted_calls: true,
            search_returns_schema: false,
        };
        let (shutdown_tx, _) = broadcast::channel(1);
        Router::new(
            Arc::new(PoolManager::new()),
            Arc::new(RwLock::new(ToolRegistry::new())),
            5,
            4000,
            make_filter_pipeline(),
            exposure,
            always_include.into_iter().map(|s| s.to_string()).collect(),
            max_tools_exposed,
            shutdown_tx,
        )
    }

    // TE6 Test 1: pinned server tools are included; non-pinned tools are excluded.
    #[test]
    fn test_exposure_filter_pinned_server_only() {
        let router = make_exposure_router(vec!["test-server"], vec![], 5);

        let backend = vec![
            fake_server_tools("test-server", 3),
            fake_server_tools("other-server", 10),
        ];

        let exposed = router.apply_exposure_filter(&backend);

        // Exactly 3 tools from test-server (cap=5 but only 3 available)
        assert_eq!(
            exposed.len(),
            3,
            "Should expose exactly 3 test-server tools, got {}",
            exposed.len()
        );
        for tool in &exposed {
            let name = tool["name"].as_str().unwrap_or("");
            assert!(
                name.starts_with("test-server/"),
                "All exposed tools should be from test-server, got: {}",
                name
            );
        }
    }

    // TE6 Test 2: always_include fills slots before pinned-server tools.
    #[test]
    fn test_exposure_filter_always_include_fills_first() {
        // server-b has a "priority_tool" that must always appear first
        let router = make_exposure_router(
            vec!["server-a"],
            vec!["server-b/priority_tool"],
            3, // cap = 3 backend tools total
        );

        // server-a: 5 tools; server-b: 2 tools, one is "priority_tool"
        let server_a = fake_server_tools("server-a", 5);
        let server_b_tools: Vec<Value> = vec![
            json!({"name": "priority_tool", "description": "High-priority tool"}),
            json!({"name": "other_tool",    "description": "Low-priority tool"}),
        ];
        let backend = vec![server_a, ("server-b".to_string(), server_b_tools)];

        let exposed = router.apply_exposure_filter(&backend);

        // Cap is 3: 1 from always_include + 2 from server-a
        assert_eq!(
            exposed.len(),
            3,
            "Should expose exactly 3 backend tools (cap=3), got {}",
            exposed.len()
        );

        // priority_tool must be present
        let names: Vec<&str> = exposed
            .iter()
            .map(|t| t["name"].as_str().unwrap_or(""))
            .collect();
        assert!(
            names.contains(&"server-b/priority_tool"),
            "priority_tool must be in exposed list, got: {:?}",
            names
        );

        // priority_tool should appear first (always_include pass runs before pinned pass)
        assert_eq!(
            names[0], "server-b/priority_tool",
            "priority_tool should be the first exposed tool"
        );
    }

    // TE6 Test 3: zero-config (no pinned, no always_include) → all tools pass through.
    #[test]
    fn test_exposure_filter_zero_config_passthrough() {
        let router = make_exposure_router(vec![], vec![], 100);

        let backend = vec![
            fake_server_tools("server-x", 3),
            fake_server_tools("server-y", 2),
        ];

        let exposed = router.apply_exposure_filter(&backend);

        assert_eq!(
            exposed.len(),
            5,
            "All 5 tools should pass through in zero-config mode, got {}",
            exposed.len()
        );
    }

    // TE6 Test 4: max_tools_exposed cap is enforced for pinned servers.
    #[test]
    fn test_exposure_cap_enforced() {
        let router = make_exposure_router(vec!["server-a"], vec![], 2);

        let backend = vec![fake_server_tools("server-a", 10)];

        let exposed = router.apply_exposure_filter(&backend);

        assert_eq!(
            exposed.len(),
            2,
            "Cap of 2 should be enforced; got {}",
            exposed.len()
        );
    }

    // TE6 Test 5: scp_search respects the search_returns_schema flag.
    #[tokio::test]
    async fn test_scp_search_returns_schema_conditional() {
        use scp_index::ToolEntry;

        // --- Without schema ---
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        {
            let mut reg = tool_registry.write().await;
            reg.register_tools(
                "test-server",
                vec![ToolEntry {
                    original_name: "my_tool".to_string(),
                    qualified_name: "test-server/my_tool".to_string(),
                    server_name: "test-server".to_string(),
                    description: Some("A test tool for schema flag testing".to_string()),
                    input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
                    tags: vec![],
                    avg_response_tokens: 0.0,
                    call_count: 0,
                }],
            );
        }

        // Router with search_returns_schema = false (default)
        let (shutdown_tx_no_schema, _) = broadcast::channel(1);
        let router_no_schema = Router::new(
            Arc::new(PoolManager::new()),
            tool_registry.clone(),
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig {
                pinned_servers: vec![],
                allow_unlisted_calls: true,
                search_returns_schema: false,
            },
            vec![],
            50,
            shutdown_tx_no_schema,
        );

        let search_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "scp_search",
                "arguments": {
                    "query": "schema",
                    "limit": 5
                }
            })),
        };

        let resp = router_no_schema.route(search_req.clone(), None).await;
        assert!(resp.result.is_some(), "scp_search should return a result");

        let text = resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let parsed: Value = serde_json::from_str(&text).expect("Should be valid JSON");
        let results = parsed["results"].as_array().expect("Should have results");

        // When search_returns_schema = false, no result should have inputSchema
        for item in results {
            assert!(
                item.get("inputSchema").is_none(),
                "search_returns_schema=false: result should NOT have inputSchema, got: {}",
                item
            );
        }

        // --- With schema ---
        let (shutdown_tx_with_schema, _) = broadcast::channel(1);
        let router_with_schema = Router::new(
            Arc::new(PoolManager::new()),
            tool_registry.clone(),
            5,
            4000,
            make_filter_pipeline(),
            scp_core::config::ExposureConfig {
                pinned_servers: vec![],
                allow_unlisted_calls: true,
                search_returns_schema: true,
            },
            vec![],
            50,
            shutdown_tx_with_schema,
        );

        let search_req2 = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(2)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "scp_search",
                "arguments": {
                    "query": "schema",
                    "limit": 5
                }
            })),
        };

        let resp2 = router_with_schema.route(search_req2, None).await;
        assert!(resp2.result.is_some(), "scp_search should return a result");

        let text2 = resp2.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let parsed2: Value = serde_json::from_str(&text2).expect("Should be valid JSON");
        let results2 = parsed2["results"].as_array().expect("Should have results");

        // With search_returns_schema = true, non-null schemas should be present
        assert!(
            !results2.is_empty(),
            "Should have at least one result for the schema query"
        );
        let first = &results2[0];
        assert!(
            first.get("inputSchema").is_some(),
            "search_returns_schema=true: result should have inputSchema, got: {}",
            first
        );
    }
}
