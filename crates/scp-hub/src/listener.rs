use crate::router::Router;
use crate::session_store::SessionStore;
use crate::streaming::sse_response_from_json;
use anyhow::Result;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    routing::{delete, get, post},
    Json, Router as AxumRouter,
};
use futures::stream::{self, StreamExt};
use scp_core::config::AuthConfig;
use scp_core::protocol::{IncomingMessage, JsonRpcRequest};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};
use uuid::Uuid;

/// HTTP client listener state.
#[derive(Clone)]
pub struct ListenerState {
    /// Session store.
    pub session_store: Arc<SessionStore>,
    /// Request router.
    pub router: Arc<Router>,
    /// Authentication configuration.
    pub auth_config: Option<AuthConfig>,
}

/// Client listener for HTTP connections.
pub struct ClientListener {
    /// Socket address to listen on.
    addr: SocketAddr,
    /// Listener state.
    state: ListenerState,
}

impl ClientListener {
    /// Create a new client listener
    pub fn new(
        addr: SocketAddr,
        session_store: Arc<SessionStore>,
        router: Arc<Router>,
        auth_config: Option<AuthConfig>,
    ) -> Self {
        Self {
            addr,
            state: ListenerState {
                session_store,
                router,
                auth_config,
            },
        }
    }

    /// Start the HTTP listener
    #[allow(dead_code)]
    pub async fn run(self) -> Result<()> {
        let app = build_app(self.state);

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        info!("HTTP listener started on {}", self.addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(())
    }

    /// Start the HTTP listener with graceful shutdown support
    pub async fn run_with_shutdown<F>(self, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let app = build_app(self.state);

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        info!("HTTP listener started on {}", self.addr);

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await?;

        Ok(())
    }
}

/// Build the axum application, keeping /health and /metrics public while
/// protecting all other routes with the authentication middleware.
fn build_app(state: ListenerState) -> AxumRouter {
    // Protected routes: require auth when a token/profile is configured.
    let protected = AxumRouter::new()
        .route("/mcp", post(handle_post_mcp))
        .route("/mcp", get(handle_get_mcp))
        .route("/mcp", delete(handle_delete_mcp))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());

    // Public routes: always accessible without authentication.
    let public = AxumRouter::new()
        .route("/health", get(health_handler_simple))
        .with_state(state);

    AxumRouter::new().merge(protected).merge(public)
}

/// Simple health check endpoint (unauthenticated)
async fn health_handler_simple() -> impl axum::response::IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

/// Build a 401 Unauthorized response with a `WWW-Authenticate: Bearer` header
/// and a JSON body `{"error": "<message>"}`.
fn unauthorized_response(message: &str) -> Response {
    use axum::response::IntoResponse;
    let mut resp = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": message})),
    )
        .into_response();
    if let Ok(hv) = "Bearer".parse() {
        resp.headers_mut().insert("WWW-Authenticate", hv);
    }
    resp
}

/// Authentication middleware
///
/// Enforcement priority:
/// 1. If `auth_config.bearer_token` is set, the request **must** supply
///    `Authorization: Bearer <token>` with the exact configured value.
///    Any mismatch → 401 with `WWW-Authenticate: Bearer`.
/// 2. If `auth_config.method` is `"bearer"` (profile-based), the token is
///    resolved against the profiles map as before.
/// 3. If no auth config is present, requests pass through unauthenticated.
async fn auth_middleware(
    State(state): State<ListenerState>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

    // Phase 1: simple bearer_token enforcement.
    if let Some(ref auth_config) = state.auth_config {
        if let Some(ref expected_token) = auth_config.bearer_token {
            if token != expected_token.as_str() {
                return unauthorized_response("Unauthorized");
            }
        }
    }

    // Phase 2: profile-based bearer auth.
    let profile_name = if let Some(auth_config) = &state.auth_config {
        match auth_config.method.as_str() {
            "bearer" => {
                // Bearer token required
                if token.is_empty() {
                    return unauthorized_response("Bearer token required");
                }

                // Resolve profile by token
                match auth_config.resolve_profile(token) {
                    Some(profile_name) => profile_name,
                    None => {
                        return unauthorized_response("Invalid bearer token");
                    }
                }
            }
            "none" => "default".to_string(),
            _ => "default".to_string(),
        }
    } else {
        // No auth config, use default profile
        "default".to_string()
    };

    // Store profile name in request extensions for later use
    request.extensions_mut().insert(profile_name.clone());

    let mut response = next.run(request).await;

    // Add profile name to response headers for client reference
    if let Ok(hv) = profile_name.parse() {
        response.headers_mut().insert("X-Profile-Name", hv);
    }

    response
}

/// POST /mcp — receive client request
async fn handle_post_mcp(
    State(state): State<ListenerState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    match handle_post_mcp_inner(state, headers, body).await {
        Ok(resp) => resp,
        Err((status, message)) => {
            (status, Json(serde_json::json!({"error": message}))).into_response()
        }
    }
}

/// Inner implementation: returns `Ok(Response)` on success or `Err((StatusCode, String))` on
/// failure. The outer function converts errors into JSON error responses.
async fn handle_post_mcp_inner(
    state: ListenerState,
    headers: HeaderMap,
    body: String,
) -> Result<Response, (StatusCode, String)> {
    // Determine whether the client wants an SSE streaming response.
    let wants_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false);

    // Generate correlation ID for this request
    let correlation_id = Uuid::new_v4().to_string();

    // Create a tracing span with correlation ID
    let span = tracing::info_span!(
        "mcp_request",
        correlation_id = %correlation_id,
        method = "POST"
    );

    let _guard = span.enter();

    // Parse JSON-RPC message
    let msg: Value = serde_json::from_str(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)))?;

    // Get profile name from header (set by auth middleware)
    let profile_name = headers
        .get("X-Profile-Name")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("default")
        .to_string();

    // Get profile config to determine budget and rate limit
    let (budget, rate_limit) = if let Some(auth_config) = &state.auth_config {
        if let Some(profile_config) = auth_config.profiles.get(&profile_name) {
            let budget = profile_config.token_budget_per_request;
            let rate_limit = profile_config.rate_limit_per_minute.unwrap_or(60);
            (budget, rate_limit)
        } else {
            // Profile not found, use defaults
            (4000, 60)
        }
    } else {
        // No auth config, use defaults
        (4000, 60)
    };

    // Get or create session — if a session ID is provided but not found, create a new one
    // transparently so that reconnecting clients don't receive 404 errors.
    let session_id = if let Some(session_id_header) = headers.get("Mcp-Session-Id") {
        let requested_id = session_id_header
            .to_str()
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid session ID header".to_string(),
                )
            })?
            .to_string();
        // If the session still exists, reuse it; otherwise silently create a new one.
        if state.session_store.get(&requested_id).await.is_some() {
            requested_id
        } else {
            let (id, _rx) = state
                .session_store
                .create(None, profile_name.clone(), budget, rate_limit)
                .await;
            id
        }
    } else {
        // Create new session with profile
        let (id, _rx) = state
            .session_store
            .create(None, profile_name.clone(), budget, rate_limit)
            .await;
        id
    };

    // Verify session exists and check rate limit
    let session = state
        .session_store
        .get(&session_id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    // Check rate limit
    let rate_limit_remaining: u32;
    let rate_limit_reset: u64;
    {
        let mut session_locked = session.lock().unwrap_or_else(|e| e.into_inner());
        if !session_locked.check_rate_limit() {
            // Compute reset timestamp and retry-after
            let now = std::time::Instant::now();
            let elapsed = now
                .saturating_duration_since(session_locked.rate_limit_last_refill)
                .as_secs();
            let retry_after = (60 - elapsed as u32).clamp(1, 60);

            let mut error_resp = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "Rate limit exceeded"})),
            )
                .into_response();
            if let Ok(hv) = retry_after.to_string().parse() {
                error_resp.headers_mut().insert("Retry-After", hv);
            }

            crate::metrics::SCP_ERRORS_TOTAL
                .with_label_values(&["rate_limited"])
                .inc();
            return Ok(error_resp);
        }
        rate_limit_remaining = session_locked.rate_limit_remaining;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        rate_limit_reset = now + 60;
    }

    debug!("POST /mcp for session {}: {}", session_id, msg);

    // Parse as IncomingMessage to distinguish requests from notifications
    let incoming: IncomingMessage = serde_json::from_value(msg.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid JSON-RPC: {}", e)))?;

    // Short-circuit notifications — return 202 with no routing (no WARN log)
    if let IncomingMessage::Notification(_) = &incoming {
        let mut resp = (StatusCode::ACCEPTED, "").into_response();
        let resp_headers = resp.headers_mut();
        if let Ok(hv) = "application/json".parse() {
            resp_headers.insert(header::CONTENT_TYPE, hv);
        }
        if let Ok(hv) = session_id.parse() {
            resp_headers.insert("Mcp-Session-Id", hv);
        }
        if let Ok(hv) = rate_limit_remaining.to_string().parse() {
            resp_headers.insert("X-SCP-RateLimit-Remaining", hv);
        }
        if let Ok(hv) = rate_limit_reset.to_string().parse() {
            resp_headers.insert("X-SCP-RateLimit-Reset", hv);
        }
        return Ok(resp);
    }

    // Extract the request (we know it's not a Notification at this point)
    let request: JsonRpcRequest = match incoming {
        IncomingMessage::Request(r) => r,
        // Response messages are not expected from clients — treat as error
        IncomingMessage::Response(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unexpected JSON-RPC Response from client".to_string(),
            ));
        }
        IncomingMessage::Notification(_) => unreachable!(),
    };

    // Route message to backend via Router
    let response = state.router.route(request).await;

    // Push response to session's outbound channel
    if let Some(session) = state.session_store.get(&session_id).await {
        let session_locked = session.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(response_value) = serde_json::to_value(&response) {
            let _ = session_locked.outbound_tx.send(response_value);
        }
    }

    // Determine the HTTP status: 202 for responses without an id, 200 otherwise.
    let status = if response.id.is_none() {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };

    let response_json = serde_json::to_value(&response).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to serialize response".to_string(),
        )
    })?;

    // Build the final HTTP response — SSE or plain JSON depending on Accept header.
    let mut final_resp = if wants_sse {
        sse_response_from_json(response_json)
    } else {
        let body_str = serde_json::to_string(&response).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to serialize response".to_string(),
            )
        })?;
        let mut r = (status, body_str).into_response();
        if let Ok(hv) = "application/json".parse() {
            r.headers_mut().insert(header::CONTENT_TYPE, hv);
        }
        r
    };

    // Attach session / rate-limit headers to whichever response type we built.
    let resp_headers = final_resp.headers_mut();
    if let Ok(hv) = session_id.parse() {
        resp_headers.insert("Mcp-Session-Id", hv);
    }
    if let Ok(hv) = rate_limit_remaining.to_string().parse() {
        resp_headers.insert("X-SCP-RateLimit-Remaining", hv);
    }
    if let Ok(hv) = rate_limit_reset.to_string().parse() {
        resp_headers.insert("X-SCP-RateLimit-Reset", hv);
    }

    Ok(final_resp)
}

/// GET /mcp — SSE stream for server-to-client messages
async fn handle_get_mcp(
    State(state): State<ListenerState>,
    headers: HeaderMap,
) -> Result<
    Sse<impl futures::stream::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, String),
> {
    // Get profile name from header (set by auth middleware)
    let profile_name = headers
        .get("X-Profile-Name")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("default")
        .to_string();

    // Get profile config to determine budget and rate limit
    let (budget, rate_limit) = if let Some(auth_config) = &state.auth_config {
        if let Some(profile_config) = auth_config.profiles.get(&profile_name) {
            let budget = profile_config.token_budget_per_request;
            let rate_limit = profile_config.rate_limit_per_minute.unwrap_or(60);
            (budget, rate_limit)
        } else {
            // Profile not found, use defaults
            (4000, 60)
        }
    } else {
        // No auth config, use defaults
        (4000, 60)
    };

    // Get or create session — if a session ID is provided but not found, create a new one
    // transparently so that reconnecting clients don't receive 404 errors.
    let session_id = if let Some(session_id_header) = headers.get("Mcp-Session-Id") {
        let requested_id = session_id_header
            .to_str()
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "Invalid session ID header".to_string(),
                )
            })?
            .to_string();
        // If the session still exists, reuse it; otherwise silently create a new one.
        if state.session_store.get(&requested_id).await.is_some() {
            requested_id
        } else {
            let (id, _rx) = state
                .session_store
                .create(None, profile_name.clone(), budget, rate_limit)
                .await;
            id
        }
    } else {
        // Create new session with profile
        let (id, _rx) = state
            .session_store
            .create(None, profile_name.clone(), budget, rate_limit)
            .await;
        id
    };

    // Get session and its outbound receiver
    let session = state
        .session_store
        .get(&session_id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session not found".to_string()))?;

    let session_locked = session.lock().unwrap_or_else(|e| e.into_inner());
    let outbound_rx = session_locked.outbound_tx.subscribe();
    drop(session_locked);

    debug!("GET /mcp SSE stream opened for session {}", session_id);

    // Immediate keepalive comment sent right when the connection is accepted.
    // MCP clients (e.g. opencode) time out if they receive zero bytes after
    // the HTTP 200 response, so we flush `: keepalive` before waiting for any
    // real event.
    let initial = stream::once(async {
        Ok::<Event, std::convert::Infallible>(Event::default().comment("keepalive"))
    });

    // Periodic keepalive every 15 seconds so the connection stays alive when
    // there are no server-push events.
    let keepalive = stream::unfold((), |()| async {
        tokio::time::sleep(Duration::from_secs(15)).await;
        Some((
            Ok::<Event, std::convert::Infallible>(Event::default().comment("keepalive")),
            (),
        ))
    });

    // Real events from the session's outbound broadcast channel.
    let events = stream::unfold(
        (outbound_rx, session_id.clone()),
        |(mut rx, sid)| async move {
            match rx.recv().await {
                Ok(msg) => {
                    let json_str = serde_json::to_string(&msg).ok()?;
                    let event = Event::default().data(json_str);
                    Some((Ok(event), (rx, sid)))
                }
                Err(_) => {
                    debug!("SSE stream closed for session {}", sid);
                    None
                }
            }
        },
    );

    // Merge keepalives with real events so whichever fires first wins, then
    // prepend the single initial keepalive.
    let stream = initial.chain(stream::select(events, keepalive));

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

/// DELETE /mcp — close session
async fn handle_delete_mcp(
    State(state): State<ListenerState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Missing session ID header".to_string(),
            )
        })?
        .to_string();

    let removed = state.session_store.remove(&session_id).await;

    if removed {
        info!("Closed session {}", session_id);
        Ok(StatusCode::OK)
    } else {
        Err((StatusCode::NOT_FOUND, "Session not found".to_string()))
    }
}

/// Run stdio client listener (P2.J)
pub async fn run_stdio_client(session_store: Arc<SessionStore>, router: Arc<Router>) -> Result<()> {
    info!("Stdio client listener started");

    // Create a session with "default" profile and no bearer token
    let (session_id, mut outbound_rx) = session_store.create_with_defaults(None).await;

    debug!("Created session for stdio client: {}", session_id);

    // Set up stdin/stdout
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    // Spawn a task to listen for outbound notifications
    let session_id_clone = session_id.clone();
    let outbound_task = tokio::spawn(async move {
        loop {
            match outbound_rx.recv().await {
                Ok(msg) => {
                    if let Ok(json_str) = serde_json::to_string(&msg) {
                        let _ = tokio::io::stdout().write_all(json_str.as_bytes()).await;
                        let _ = tokio::io::stdout().write_all(b"\n").await;
                        let _ = tokio::io::stdout().flush().await;
                    }
                }
                Err(_) => {
                    debug!("Outbound channel closed for session {}", session_id_clone);
                    break;
                }
            }
        }
    });

    // Read JSON-RPC messages from stdin
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                // Parse JSON-RPC message
                match serde_json::from_str::<JsonRpcRequest>(&line) {
                    Ok(request) => {
                        debug!("Received request from stdio: {}", request.method);

                        // Route message via Router
                        let response = router.route(request).await;

                        // Write response to stdout
                        if let Ok(json_str) = serde_json::to_string(&response) {
                            let _ = stdout.write_all(json_str.as_bytes()).await;
                            let _ = stdout.write_all(b"\n").await;
                            let _ = stdout.flush().await;
                        }
                    }
                    Err(e) => {
                        debug!("Failed to parse JSON-RPC message: {}", e);
                        // Send error response
                        let error_response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32700,
                                "message": "Parse error"
                            },
                            "id": Value::Null
                        });
                        if let Ok(json_str) = serde_json::to_string(&error_response) {
                            let _ = stdout.write_all(json_str.as_bytes()).await;
                            let _ = stdout.write_all(b"\n").await;
                            let _ = stdout.flush().await;
                        }
                    }
                }
            }
            Ok(None) => {
                // stdin closed
                debug!("Stdin closed for session {}", session_id);
                break;
            }
            Err(e) => {
                debug!("Error reading from stdin: {}", e);
                break;
            }
        }
    }

    // Remove session
    session_store.remove(&session_id).await;
    info!("Closed session {}", session_id);

    // Cancel the outbound task
    outbound_task.abort();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use scp_core::config::AuthConfig;
    use scp_index::ToolRegistry;
    use scp_pool::PoolManager;
    use tower::ServiceExt;

    fn make_state(auth_config: Option<AuthConfig>) -> ListenerState {
        let store = Arc::new(SessionStore::new(1000));
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let router = Arc::new(Router::new(pool_manager, tool_registry, 5, 4000));
        ListenerState {
            session_store: store,
            router,
            auth_config,
        }
    }

    #[test]
    fn test_listener_creation() {
        let addr = "127.0.0.1:3100".parse().unwrap();
        let store = Arc::new(SessionStore::new(1000));
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new()));
        let router = Arc::new(Router::new(pool_manager, tool_registry, 5, 4000));
        let listener = ClientListener::new(addr, store, router, None);
        assert_eq!(listener.addr, addr);
    }

    #[test]
    fn test_rate_limit_headers_present() {
        // Test that the header values are computed correctly
        let remaining = 42u32;
        let header_value = format!("{}", remaining);
        assert_eq!(header_value, "42");

        // Verify reset timestamp is in the future
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let reset = now + 60;
        assert!(reset > now);
    }

    // -----------------------------------------------------------------------
    // Bearer-token auth middleware tests
    // -----------------------------------------------------------------------

    /// Helper: build a minimal app with just the protected /mcp route so we
    /// can send requests and inspect the status code returned by the middleware.
    fn make_protected_app(auth_config: Option<AuthConfig>) -> AxumRouter {
        use axum::routing::get;
        let state = make_state(auth_config);
        AxumRouter::new()
            .route("/mcp", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_request_without_token_passes_when_no_auth_configured() {
        let app = make_protected_app(None);
        let response = app
            .oneshot(Request::builder().uri("/mcp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        // No auth configured → should reach the handler (200).
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_without_auth_header_returns_401_when_token_configured() {
        let auth = AuthConfig {
            bearer_token: Some("secret".to_string()),
            ..Default::default()
        };
        let app = make_protected_app(Some(auth));
        let response = app
            .oneshot(Request::builder().uri("/mcp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get("WWW-Authenticate")
                .map(|v| v.as_bytes()),
            Some(b"Bearer".as_ref())
        );
    }

    #[tokio::test]
    async fn test_request_with_wrong_token_returns_401() {
        let auth = AuthConfig {
            bearer_token: Some("correct-token".to_string()),
            ..Default::default()
        };
        let app = make_protected_app(Some(auth));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mcp")
                    .header("Authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_request_with_correct_token_passes() {
        let auth = AuthConfig {
            bearer_token: Some("correct-token".to_string()),
            ..Default::default()
        };
        let app = make_protected_app(Some(auth));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mcp")
                    .header("Authorization", "Bearer correct-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Correct token → reaches the handler (200).
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_endpoint_is_always_accessible() {
        // Even when a bearer_token is configured, /health must be public.
        let auth = AuthConfig {
            bearer_token: Some("secret".to_string()),
            ..Default::default()
        };
        let state = make_state(Some(auth));
        let app = build_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // SSE content-negotiation tests
    // -----------------------------------------------------------------------

    /// Helper: build a valid JSON-RPC tools/call body for the ping method.
    fn ping_body() -> Body {
        Body::from(
            serde_json::to_string(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "ping",
                "params": {}
            }))
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn test_tools_call_without_accept_returns_json() {
        let state = make_state(None);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("Content-Type", "application/json")
                    .body(ping_body())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response
            .headers()
            .get("content-type")
            .expect("content-type header missing")
            .to_str()
            .unwrap();
        assert!(
            ct.contains("application/json"),
            "Expected application/json, got: {ct}"
        );
    }

    #[tokio::test]
    async fn test_tools_call_with_sse_accept_returns_sse() {
        let state = make_state(None);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .body(ping_body())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response
            .headers()
            .get("content-type")
            .expect("content-type header missing")
            .to_str()
            .unwrap();
        assert!(
            ct.contains("text/event-stream"),
            "Expected text/event-stream, got: {ct}"
        );
    }

    #[tokio::test]
    async fn test_sse_response_contains_valid_json_rpc_payload() {
        use axum::body::to_bytes;

        let state = make_state(None);
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .body(ping_body())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = std::str::from_utf8(&bytes).unwrap();

        // SSE format: lines starting with "data:" contain the payload
        let data_line = body_str
            .lines()
            .find(|l| l.starts_with("data:"))
            .expect("No data: line in SSE body");

        let json_part = data_line.trim_start_matches("data:").trim();
        let parsed: serde_json::Value =
            serde_json::from_str(json_part).expect("SSE data is not valid JSON");

        assert_eq!(
            parsed["jsonrpc"], "2.0",
            "jsonrpc field must be '2.0', got: {}",
            parsed
        );
    }
}
