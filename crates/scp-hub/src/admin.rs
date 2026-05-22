#![allow(unused_imports)]

use crate::server_manager::{ServerManager, ServerStatus};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use scp_core::config::ServerConfig;
use serde::Serialize;
use tracing::info;

/// Admin API state
#[derive(Clone)]
pub struct AdminState {
    pub server_manager: ServerManager,
    #[allow(dead_code)]
    pub auth_token: Option<String>,
}

/// Health response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub servers: usize,
    pub healthy: usize,
}

/// Server list response
#[derive(Debug, Serialize)]
pub struct ServerListResponse {
    pub servers: Vec<ServerStatusResponse>,
}

/// Server status response
#[derive(Debug, Serialize)]
pub struct ServerStatusResponse {
    pub name: String,
    pub state: String,
    pub tool_count: usize,
    pub enabled: bool,
}

impl From<ServerStatus> for ServerStatusResponse {
    fn from(status: ServerStatus) -> Self {
        Self {
            name: status.name,
            state: status.state.to_string(),
            tool_count: status.tool_count,
            enabled: status.enabled,
        }
    }
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
        .with_state(state)
}

/// GET /health
async fn health_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let servers = state.server_manager.list_servers().await;
    let healthy = servers.iter().filter(|s| s.enabled).count();

    Json(HealthResponse {
        status: "ok".to_string(),
        servers: servers.len(),
        healthy,
    })
}

/// GET /servers
async fn list_servers_handler(State(state): State<AdminState>) -> impl IntoResponse {
    let servers = state.server_manager.list_servers().await;
    let response = ServerListResponse {
        servers: servers.into_iter().map(|s| s.into()).collect(),
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

/// POST /config/reload
async fn reload_config_handler(State(_state): State<AdminState>) -> impl IntoResponse {
    // Placeholder for config reload (implemented in P1.I)
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "ok", "message": "Config reload not yet implemented"})),
    )
}

/// Start admin API server
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
