#![allow(unused_imports)]

use crate::server_manager::{ServerManager, ServerStatus};
use crate::session_store::{SessionStore, SessionSummary};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use scp_core::config::ServerConfig;
use scp_index::ToolRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Admin API state.
#[derive(Clone)]
pub struct AdminState {
    /// Server manager.
    pub server_manager: ServerManager,
    /// Optional authentication token.
    #[allow(dead_code)]
    pub auth_token: Option<String>,
    /// Optional session store.
    pub session_store: Option<Arc<SessionStore>>,
    /// Optional tool registry.
    pub tool_registry: Option<Arc<RwLock<ToolRegistry>>>,
    /// Optional path to the config file for hot-reload.
    pub config_path: Option<std::path::PathBuf>,
}

/// Health response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall status.
    pub status: String,
    /// Total number of servers.
    pub servers: usize,
    /// Number of healthy servers.
    pub healthy: usize,
    /// Number of active sessions.
    pub sessions: usize,
}

/// Server list response.
#[derive(Debug, Serialize)]
pub struct ServerListResponse {
    /// List of server statuses.
    pub servers: Vec<ServerStatusResponse>,
}

/// Server status response.
#[derive(Debug, Serialize)]
pub struct ServerStatusResponse {
    /// Server name.
    pub name: String,
    /// Server state.
    pub state: String,
    /// Number of tools provided by this server.
    pub tool_count: usize,
    /// Whether the server is enabled.
    pub enabled: bool,
    /// HTTP headers configured for this server, with sensitive values redacted.
    pub headers: HashMap<String, String>,
}

impl From<ServerStatus> for ServerStatusResponse {
    fn from(status: ServerStatus) -> Self {
        Self {
            name: status.name,
            state: status.state.to_string(),
            tool_count: status.tool_count,
            enabled: status.enabled,
            headers: HashMap::new(),
        }
    }
}

/// Sensitive header-name fragments (case-insensitive).
const SENSITIVE_HEADER_PATTERNS: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-token",
    "token",
    "secret",
    "password",
];

/// Return a copy of `headers` with values for sensitive keys replaced by
/// `"[REDACTED]"`.  A key is considered sensitive when its lower-case form
/// contains any of the [`SENSITIVE_HEADER_PATTERNS`].
pub fn redact_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| {
            let lower = k.to_lowercase();
            let redacted = SENSITIVE_HEADER_PATTERNS
                .iter()
                .any(|pat| lower.contains(pat));
            (
                k.clone(),
                if redacted {
                    "[REDACTED]".to_string()
                } else {
                    v.clone()
                },
            )
        })
        .collect()
}

/// Tool info response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Qualified tool name.
    pub qualified_name: String,
    /// Server providing the tool.
    pub server: String,
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// Tool tags.
    pub tags: Vec<String>,
    /// Number of times the tool has been called.
    pub call_count: u64,
}

/// Create admin API router
pub fn create_admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route(
            "/servers",
            get(list_servers_handler).post(add_server_handler),
        )
        .route(
            "/servers/:name",
            put(update_server_handler).delete(delete_server_handler),
        )
        .route("/servers/:name/disable", post(disable_server_handler))
        .route("/servers/:name/enable", post(enable_server_handler))
        .route("/config/reload", post(reload_config_handler))
        .route("/admin/sessions", get(list_sessions_handler))
        .route(
            "/admin/sessions/:id",
            get(get_session_handler).delete(delete_session_handler),
        )
        .route("/tools", get(list_tools_handler))
        .route("/admin/metrics", get(metrics_handler))
        .route("/metrics", get(prometheus_metrics_handler))
        .with_state(state)
}

/// GET /health
async fn health_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let servers = state.server_manager.list_servers().await;
    let healthy = servers.iter().filter(|s| s.enabled).count();
    let total_servers = servers.len();

    // Determine status based on server health
    let status = if total_servers == 0 {
        // No servers configured, consider healthy
        "ok".to_string()
    } else if healthy == total_servers {
        // All servers healthy
        "ok".to_string()
    } else if healthy > 0 {
        // Some servers healthy, some down
        "degraded".to_string()
    } else {
        // All servers down
        "error".to_string()
    };

    // Get session count
    let sessions = if let Some(session_store) = &state.session_store {
        session_store.list().await.len()
    } else {
        0
    };

    Json(HealthResponse {
        status,
        servers: total_servers,
        healthy,
        sessions,
    })
}

/// GET /servers
async fn list_servers_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let servers = state.server_manager.list_servers().await;
    let configs = state.server_manager.list_configs().await;

    let response = ServerListResponse {
        servers: servers
            .into_iter()
            .map(|s| {
                let raw_headers = configs
                    .iter()
                    .find(|c| c.name == s.name)
                    .map(|c| &c.headers)
                    .cloned()
                    .unwrap_or_default();
                let mut resp: ServerStatusResponse = s.into();
                resp.headers = redact_headers(&raw_headers);
                resp
            })
            .collect(),
    };
    Json(response)
}

/// POST /servers
async fn add_server_handler(
    State(state): State<AdminState>,
    Json(config): Json<ServerConfig>,
) -> impl IntoResponse {
    match state.server_manager.add_server(config).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"status": "ok"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// PUT /servers/:name
async fn update_server_handler(
    State(state): State<AdminState>,
    Path(name): Path<String>,
    Json(config): Json<ServerConfig>,
) -> impl IntoResponse {
    match state.server_manager.update_server(&name, config).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /servers/:name
async fn delete_server_handler(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.server_manager.remove_server(&name).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /servers/:name/disable
async fn disable_server_handler(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.server_manager.disable_server(&name).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /servers/:name/enable
async fn enable_server_handler(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.server_manager.enable_server(&name).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Returns true when two server configs differ in ways that require a process restart.
fn server_config_changed(old: &ServerConfig, new: &ServerConfig) -> bool {
    old.transport != new.transport
        || old.command != new.command
        || old.args != new.args
        || old.url != new.url
        || old.env != new.env
        || old.headers != new.headers
        || old.enabled != new.enabled
}

/// POST /config/reload
async fn reload_config_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let Some(ref config_path) = state.config_path else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "Config path not configured"})),
        )
            .into_response();
    };

    let new_config = match scp_core::config::load_config(config_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Failed to reload config: {}", e)})),
            )
                .into_response();
        }
    };

    // Snapshot the currently running server configs before we mutate anything.
    let current_configs = state.server_manager.list_configs().await;

    let current_map: std::collections::HashMap<&str, &ServerConfig> = current_configs
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let new_map: std::collections::HashMap<&str, &ServerConfig> = new_config
        .servers
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let mut added = 0usize;
    let mut removed = 0usize;
    let mut restarted = 0usize;

    // Removed servers: present in current but absent from new config.
    for name in current_map.keys() {
        if !new_map.contains_key(name) {
            if let Err(e) = state.server_manager.remove_server(name).await {
                info!("reload: failed to remove server {}: {}", name, e);
            } else {
                removed += 1;
                info!("reload: removed server {}", name);
            }
        }
    }

    // Added or changed servers.
    for (name, new_cfg) in &new_map {
        if let Some(old_cfg) = current_map.get(name) {
            if server_config_changed(old_cfg, new_cfg) {
                // Config changed — remove then re-add to restart the process.
                if let Err(e) = state.server_manager.remove_server(name).await {
                    info!(
                        "reload: failed to remove server {} for restart: {}",
                        name, e
                    );
                    continue;
                }
                if new_cfg.enabled {
                    if let Err(e) = state.server_manager.add_server((*new_cfg).clone()).await {
                        info!("reload: failed to re-add server {}: {}", name, e);
                    } else {
                        restarted += 1;
                        info!("reload: restarted server {}", name);
                    }
                } else {
                    removed += 1;
                    info!("reload: removed disabled server {}", name);
                }
            }
            // Unchanged — no-op.
        } else {
            // New server.
            if new_cfg.enabled {
                if let Err(e) = state.server_manager.add_server((*new_cfg).clone()).await {
                    info!("reload: failed to add server {}: {}", name, e);
                } else {
                    added += 1;
                    info!("reload: added server {}", name);
                }
            }
        }
    }

    // Invalidate the tool registry so the next tools/list re-discovers all tools.
    // Each server's tools are already cleaned up by remove_server → unregister_server.
    // Clearing the shared registry handle here ensures any stale entries are purged.
    if let Some(registry) = &state.tool_registry {
        let mut reg = registry.write().await;
        *reg = scp_index::ToolRegistry::new();
        info!("reload: tool registry cleared");
    }

    info!(
        "reload: complete — added={}, removed={}, restarted={}",
        added, removed, restarted
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "added": added,
            "removed": removed,
            "restarted": restarted,
        })),
    )
        .into_response()
}

/// GET /admin/sessions/:id
async fn get_session_handler(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(session_store) = &state.session_store {
        let sessions = session_store.list().await;
        if let Some(session) = sessions.into_iter().find(|s| s.id == id) {
            (
                StatusCode::OK,
                Json(serde_json::to_value(session).unwrap_or(serde_json::json!({}))),
            )
                .into_response()
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Session not found"})),
            )
                .into_response()
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Session store unavailable"})),
        )
            .into_response()
    }
}

/// GET /admin/sessions (P2.L)
async fn list_sessions_handler(State(state): State<AdminState>) -> impl IntoResponse {
    if let Some(session_store) = &state.session_store {
        let sessions = session_store.list().await;
        Json(serde_json::json!({
            "sessions": sessions
        }))
    } else {
        Json(serde_json::json!({
            "sessions": []
        }))
    }
}

/// DELETE /admin/sessions/:id (P2.L)
async fn delete_session_handler(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(session_store) = &state.session_store {
        if session_store.remove(&id).await {
            (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))).into_response()
        } else {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Session not found"})),
            )
                .into_response()
        }
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Session store not available"})),
        )
            .into_response()
    }
}

/// GET /admin/metrics (JSON format)
async fn metrics_handler() -> impl IntoResponse {
    let errors_total = serde_json::json!({
        "tool_not_found": crate::metrics::SCP_ERRORS_TOTAL.with_label_values(&["tool_not_found"]).get(),
        "server_not_found": crate::metrics::SCP_ERRORS_TOTAL.with_label_values(&["server_not_found"]).get(),
        "pool_error": crate::metrics::SCP_ERRORS_TOTAL.with_label_values(&["pool_error"]).get(),
        "invalid_request": crate::metrics::SCP_ERRORS_TOTAL.with_label_values(&["invalid_request"]).get(),
        "rate_limited": crate::metrics::SCP_ERRORS_TOTAL.with_label_values(&["rate_limited"]).get(),
    });

    let request_duration = serde_json::json!({
        "count": crate::metrics::SCP_REQUEST_DURATION_SECONDS.get_sample_count(),
        "sum": crate::metrics::SCP_REQUEST_DURATION_SECONDS.get_sample_sum(),
    });

    Json(serde_json::json!({
        "tokens_saved_total": crate::metrics::SCP_TOKENS_SAVED_TOTAL.get(),
        "tokens_delivered_total": crate::metrics::SCP_TOKENS_DELIVERED_TOTAL.get(),
        "embedding_fallback_total": crate::metrics::SCP_EMBEDDING_FALLBACK_TOTAL.get(),
        "errors_total": errors_total,
        "pool_connections_active": crate::metrics::SCP_POOL_CONNECTIONS_ACTIVE.get(),
        "inflight_requests": crate::metrics::SCP_INFLIGHT_REQUESTS.get(),
        "request_duration_seconds": request_duration,
    }))
}

/// GET /metrics (Prometheus text format)
async fn prometheus_metrics_handler() -> impl IntoResponse {
    let body = crate::metrics::gather_metrics();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

/// GET /tools (P3.J)
async fn list_tools_handler(
    State(state): State<AdminState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if let Some(registry) = &state.tool_registry {
        let registry = registry.read().await;
        let query = params.get("q").map(|s| s.to_lowercase());

        let tools: Vec<ToolInfo> = registry
            .all_tools()
            .into_iter()
            .filter(|entry| {
                if let Some(q) = &query {
                    entry.original_name.to_lowercase().contains(q)
                        || entry
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(q))
                            .unwrap_or(false)
                } else {
                    true
                }
            })
            .map(|entry| {
                let (server, name) = if let Some(pos) = entry.qualified_name.find('.') {
                    (
                        entry.qualified_name[..pos].to_string(),
                        entry.qualified_name[pos + 1..].to_string(),
                    )
                } else {
                    (String::new(), entry.qualified_name.clone())
                };

                ToolInfo {
                    qualified_name: entry.qualified_name.clone(),
                    server,
                    name,
                    description: entry.description.clone().unwrap_or_default(),
                    tags: entry.tags.clone(),
                    call_count: entry.call_count,
                }
            })
            .collect();

        Json(tools)
    } else {
        Json(Vec::<ToolInfo>::new())
    }
}

/// Start admin API server
#[allow(dead_code)]
pub async fn start_admin_api(
    listen_addr: &str,
    listen_port: u16,
    state: AdminState,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_admin_router(state);
    let addr = format!("{}:{}", listen_addr, listen_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Admin API listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the admin API with a shutdown signal.
pub async fn start_admin_api_with_shutdown<F>(
    listen_addr: &str,
    listen_port: u16,
    state: AdminState,
    shutdown: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let app = create_admin_router(state);
    let addr = format!("{}:{}", listen_addr, listen_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Admin API listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scp_index::ToolEntry;

    fn create_test_tool(name: &str, description: &str) -> ToolEntry {
        ToolEntry {
            original_name: name.to_string(),
            qualified_name: String::new(), // Will be set by register_tools
            server_name: String::new(),
            description: Some(description.to_string()),
            input_schema: serde_json::json!({}),
            tags: vec!["test".to_string()],
            avg_response_tokens: 100.0,
            call_count: 0,
        }
    }

    #[tokio::test]
    async fn test_list_tools_empty() {
        let state = AdminState {
            server_manager: ServerManager::new(
                Arc::new(scp_pool::PoolManager::new()),
                Arc::new(RwLock::new(ToolRegistry::new())),
            ),
            auth_token: None,
            session_store: None,
            tool_registry: None,
            config_path: None,
        };

        let params = HashMap::new();
        let response = list_tools_handler(State(state), Query(params)).await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(tools.len(), 0);
    }

    #[tokio::test]
    async fn test_list_tools_with_registry() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("read_file", "Read a file from the filesystem"),
            create_test_tool("write_file", "Write a file to the filesystem"),
            create_test_tool("search", "Search for files"),
        ];
        registry.register_tools("fs", tools);

        let state = AdminState {
            server_manager: ServerManager::new(
                Arc::new(scp_pool::PoolManager::new()),
                Arc::new(RwLock::new(ToolRegistry::new())),
            ),
            auth_token: None,
            session_store: None,
            tool_registry: Some(Arc::new(RwLock::new(registry))),
            config_path: None,
        };

        let params = HashMap::new();
        let response = list_tools_handler(State(state), Query(params)).await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "read_file"));
        assert!(tools.iter().any(|t| t.name == "write_file"));
        assert!(tools.iter().any(|t| t.name == "search"));
    }

    #[tokio::test]
    async fn test_list_tools_with_filter() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("read_file", "Read a file from the filesystem"),
            create_test_tool("write_file", "Write a file to the filesystem"),
            create_test_tool("search", "Search for documents"),
        ];
        registry.register_tools("fs", tools);

        let state = AdminState {
            server_manager: ServerManager::new(
                Arc::new(scp_pool::PoolManager::new()),
                Arc::new(RwLock::new(ToolRegistry::new())),
            ),
            auth_token: None,
            session_store: None,
            tool_registry: Some(Arc::new(RwLock::new(registry))),
            config_path: None,
        };

        let mut params = HashMap::new();
        params.insert("q".to_string(), "file".to_string());
        let response = list_tools_handler(State(state), Query(params)).await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "read_file"));
        assert!(tools.iter().any(|t| t.name == "write_file"));
        assert!(!tools.iter().any(|t| t.name == "search"));
    }

    #[tokio::test]
    async fn test_list_tools_filter_by_description() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("read_file", "Read a file from the filesystem"),
            create_test_tool("write_file", "Write a file to the filesystem"),
            create_test_tool("search", "Search for files"),
        ];
        registry.register_tools("fs", tools);

        let state = AdminState {
            server_manager: ServerManager::new(
                Arc::new(scp_pool::PoolManager::new()),
                Arc::new(RwLock::new(ToolRegistry::new())),
            ),
            auth_token: None,
            session_store: None,
            tool_registry: Some(Arc::new(RwLock::new(registry))),
            config_path: None,
        };

        let mut params = HashMap::new();
        params.insert("q".to_string(), "filesystem".to_string());
        let response = list_tools_handler(State(state), Query(params)).await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "read_file"));
        assert!(tools.iter().any(|t| t.name == "write_file"));
    }

    #[tokio::test]
    async fn test_list_tools_case_insensitive_filter() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("read_file", "Read a file from the filesystem"),
            create_test_tool("write_file", "Write a file to the filesystem"),
            create_test_tool("search", "Search for documents"),
        ];
        registry.register_tools("fs", tools);

        let state = AdminState {
            server_manager: ServerManager::new(
                Arc::new(scp_pool::PoolManager::new()),
                Arc::new(RwLock::new(ToolRegistry::new())),
            ),
            auth_token: None,
            session_store: None,
            tool_registry: Some(Arc::new(RwLock::new(registry))),
            config_path: None,
        };

        let mut params = HashMap::new();
        params.insert("q".to_string(), "FILE".to_string());
        let response = list_tools_handler(State(state), Query(params)).await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let tools: Vec<ToolInfo> = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "read_file"));
        assert!(tools.iter().any(|t| t.name == "write_file"));
    }

    #[test]
    fn test_redact_headers_sensitive_keys() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer secret-token".to_string(),
        );
        headers.insert("X-Api-Key".to_string(), "my-api-key".to_string());
        headers.insert("X-Token".to_string(), "abc123".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("Accept".to_string(), "text/plain".to_string());

        let redacted = redact_headers(&headers);

        assert_eq!(redacted["Authorization"], "[REDACTED]");
        assert_eq!(redacted["X-Api-Key"], "[REDACTED]");
        assert_eq!(redacted["X-Token"], "[REDACTED]");
        assert_eq!(redacted["Content-Type"], "application/json");
        assert_eq!(redacted["Accept"], "text/plain");
    }

    #[test]
    fn test_redact_headers_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("AUTHORIZATION".to_string(), "Bearer token".to_string());
        headers.insert("x-api-key".to_string(), "key-value".to_string());
        headers.insert("My-Secret-Header".to_string(), "shh".to_string());
        headers.insert("my-password".to_string(), "p@ss".to_string());

        let redacted = redact_headers(&headers);

        assert_eq!(redacted["AUTHORIZATION"], "[REDACTED]");
        assert_eq!(redacted["x-api-key"], "[REDACTED]");
        assert_eq!(redacted["My-Secret-Header"], "[REDACTED]");
        assert_eq!(redacted["my-password"], "[REDACTED]");
    }

    #[test]
    fn test_redact_headers_empty() {
        let headers: HashMap<String, String> = HashMap::new();
        let redacted = redact_headers(&headers);
        assert!(redacted.is_empty());
    }

    #[test]
    fn test_redact_headers_no_sensitive() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("Accept".to_string(), "*/*".to_string());
        headers.insert("User-Agent".to_string(), "scp/1.0".to_string());

        let redacted = redact_headers(&headers);

        assert_eq!(redacted["Content-Type"], "application/json");
        assert_eq!(redacted["Accept"], "*/*");
        assert_eq!(redacted["User-Agent"], "scp/1.0");
    }

    #[tokio::test]
    async fn test_prometheus_metrics_endpoint() {
        // Increment one of the error counters to ensure it appears in output
        crate::metrics::SCP_ERRORS_TOTAL
            .with_label_values(&["test"])
            .inc();
        let metrics_output = crate::metrics::gather_metrics();

        assert!(metrics_output.contains("scp_tokens_saved_total"));
        assert!(metrics_output.contains("scp_tokens_delivered_total"));
        assert!(metrics_output.contains("scp_embedding_fallback_total"));
        assert!(metrics_output.contains("scp_request_duration_seconds"));
        assert!(metrics_output.contains("scp_errors_total"));
        assert!(metrics_output.contains("scp_pool_connections_active"));
    }

    #[tokio::test]
    async fn test_health_response_has_sessions() {
        let response = HealthResponse {
            status: "ok".to_string(),
            servers: 2,
            healthy: 2,
            sessions: 3,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("sessions"));
        assert!(json.contains("\"sessions\":3"));
    }

    #[tokio::test]
    async fn test_admin_metrics_extended() {
        let response = metrics_handler().await;
        let body = response.into_response().into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let metrics: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        // Verify all required fields are present
        assert!(metrics.get("tokens_saved_total").is_some());
        assert!(metrics.get("tokens_delivered_total").is_some());
        assert!(metrics.get("embedding_fallback_total").is_some());
        assert!(metrics.get("errors_total").is_some());
        assert!(metrics.get("pool_connections_active").is_some());
        assert!(metrics.get("inflight_requests").is_some());
        assert!(metrics.get("request_duration_seconds").is_some());

        // Verify errors_total has all error kinds
        let errors = metrics.get("errors_total").unwrap();
        assert!(errors.get("tool_not_found").is_some());
        assert!(errors.get("server_not_found").is_some());
        assert!(errors.get("pool_error").is_some());
        assert!(errors.get("invalid_request").is_some());
        assert!(errors.get("rate_limited").is_some());

        // Verify request_duration_seconds has count and sum
        let duration = metrics.get("request_duration_seconds").unwrap();
        assert!(duration.get("count").is_some());
        assert!(duration.get("sum").is_some());
    }
}
