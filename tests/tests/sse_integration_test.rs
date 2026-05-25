use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use scp_core::protocol::{JsonRpcRequest, RequestId};
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Minimal in-process MCP backend (axum, POST /mcp)
// ---------------------------------------------------------------------------

/// Shared state for the in-process backend.
#[derive(Clone)]
struct BackendState {
    tool_name: Arc<str>,
}

/// Handler: POST /mcp  — speaks JSON-RPC 2.0.
async fn mcp_handler(
    State(state): State<BackendState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let method = body
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_owned();
    let id = body.get("id").cloned().unwrap_or(json!(1));

    let result = match method.as_str() {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "serverInfo": { "name": "sse-test-backend", "version": "0.1.0" }
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
                    "name": state.tool_name.as_ref(),
                    "description": "A tool provided by the SSE/HTTP backend",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input": { "type": "string" }
                        }
                    }
                }]
            }
        }),
        "tools/call" => {
            let args = body
                .get("params")
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{
                        "type": "text",
                        "text": format!("called {} with {}", state.tool_name.as_ref(), args)
                    }]
                }
            })
        }
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" }
        }),
    };

    Json(result)
}

/// Bind to a free port, start the axum server in a background task,
/// and return the bound port together with a shutdown sender.
///
/// The shutdown sender is stored in the returned `BackendHandle`; dropping
/// it shuts the server down cleanly.
struct BackendHandle {
    pub port: u16,
    _shutdown: oneshot::Sender<()>,
}

impl BackendHandle {
    async fn spawn(tool_name: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        listener.set_nonblocking(true).expect("nonblocking");

        let state = BackendState {
            tool_name: Arc::from(tool_name),
        };
        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .with_state(state);

        let (tx, rx) = oneshot::channel::<()>();

        let std_listener = listener;
        tokio::spawn(async move {
            let tokio_listener =
                tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
            axum::serve(tokio_listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .ok();
        });

        // Small yield to let axum finish binding.
        tokio::time::sleep(Duration::from_millis(50)).await;

        BackendHandle {
            port,
            _shutdown: tx,
        }
    }
}

// ---------------------------------------------------------------------------
// Hub helper (extended version of HttpTestHub with HTTP backend support)
// ---------------------------------------------------------------------------

fn scp_hub_bin() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("tests/ crate must have a parent workspace directory");
    let exe_name = if cfg!(windows) {
        "scp-hub.exe"
    } else {
        "scp-hub"
    };
    workspace_root.join("target").join("debug").join(exe_name)
}

fn mock_server_bin() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_mock-mcp-server")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            let workspace_root = std::path::Path::new(manifest_dir)
                .parent()
                .expect("tests/ crate must have a parent workspace directory");
            let exe_name = if cfg!(windows) {
                "mock-mcp-server.exe"
            } else {
                "mock-mcp-server"
            };
            workspace_root.join("target").join("debug").join(exe_name)
        })
}

fn find_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

/// Spawn scp-hub configured with both a stdio backend and a streamable_http backend.
fn spawn_hub_with_http_backend(
    hub_port: u16,
    admin_port: u16,
    auth_token: &str,
    backend_port: u16,
    mock_server_path: &str,
) -> anyhow::Result<(Child, tempfile::NamedTempFile)> {
    let config_content = format!(
        r#"config_version = 1

[hub]
listen_address = "127.0.0.1"
listen_port = {hub_port}
transports = ["http"]

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000
max_tools_exposed = 20
fanout_timeout_secs = 5
max_requests_per_min = 100
burst_size = 20

[hub.auth]
method = "bearer"

[hub.auth.profiles.default]
token = "{auth_token}"
token_budget_per_request = 4000
rate_limit_per_minute = 100

[admin]
port = {admin_port}

[filter]
enabled = true
budget_strategy = "truncate"
chunking_strategy = "paragraph"
relevance_engine = "tags"

[logging]
level = "error"
format = "pretty"

[[servers]]
name = "mock"
transport = "stdio"
command = "{mock_server_path}"
args = []
sharing = "shared"
enabled = true
priority = 100

[servers.timeouts]
connect_secs = 10
request_secs = 30
health_check_secs = 5

[servers.retries]
max_attempts = 3
initial_delay_ms = 100
max_delay_ms = 5000
backoff_factor = 2.0

[[servers]]
name = "http-backend"
transport = "streamable_http"
url = "http://127.0.0.1:{backend_port}/mcp"
args = []
sharing = "shared"
enabled = true
priority = 90

[servers.timeouts]
connect_secs = 10
request_secs = 30
health_check_secs = 5

[servers.retries]
max_attempts = 3
initial_delay_ms = 100
max_delay_ms = 5000
backoff_factor = 2.0
"#,
        hub_port = hub_port,
        auth_token = auth_token,
        admin_port = admin_port,
        mock_server_path = mock_server_path.replace('\\', "\\\\"),
        backend_port = backend_port,
    );

    let mut config_file = tempfile::NamedTempFile::new()?;
    config_file.write_all(config_content.as_bytes())?;
    config_file.flush()?;
    let config_path = config_file.path().to_path_buf();

    let process = Command::new(scp_hub_bin())
        .arg("--config")
        .arg(&config_path)
        .arg("--log-level")
        .arg("error")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for hub to be ready.
    thread::sleep(Duration::from_millis(2000));
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(15);
    loop {
        if start.elapsed() > timeout {
            break;
        }
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", admin_port)).is_ok() {
            thread::sleep(Duration::from_millis(1000));
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    Ok((process, config_file))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test that a streamable_http backend is registered and appears as warm.
#[tokio::test]
async fn test_streamable_http_backend_registers_warm() {
    let backend = BackendHandle::spawn("test_tool").await;

    let hub_port = find_available_port();
    let admin_port = find_available_port();
    let auth_token = "sse-test-token";

    let mock_path = mock_server_bin();
    let mock_str = mock_path.to_str().expect("mock path").to_string();

    let (mut process, _config) =
        spawn_hub_with_http_backend(hub_port, admin_port, auth_token, backend.port, &mock_str)
            .expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Query the admin API to verify the http-backend is warm.
    let resp = client
        .get(format!("http://127.0.0.1:{}/servers", admin_port))
        .send()
        .await
        .expect("admin request");

    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.expect("parse");
    let servers = body
        .get("servers")
        .and_then(|s| s.as_array())
        .expect("servers array");

    let http_backend = servers.iter().find(|s| {
        s.get("name")
            .and_then(|n| n.as_str())
            .map(|name| name == "http-backend")
            .unwrap_or(false)
    });

    assert!(
        http_backend.is_some(),
        "http-backend not found in server list; got: {:?}",
        servers
    );

    let status = http_backend
        .unwrap()
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    assert_eq!(
        status, "warm",
        "http-backend should be warm, got: {}",
        status
    );

    process.kill().ok();
}

/// Test that tools/list fan-out reaches the HTTP backend and returns its tools.
#[tokio::test]
async fn test_streamable_http_backend_tools_appear_in_list() {
    let backend = BackendHandle::spawn("test_tool").await;

    let hub_port = find_available_port();
    let admin_port = find_available_port();
    let auth_token = "sse-test-token-2";

    let mock_path = mock_server_bin();
    let mock_str = mock_path.to_str().expect("mock path").to_string();

    let (mut process, _config) =
        spawn_hub_with_http_backend(hub_port, admin_port, auth_token, backend.port, &mock_str)
            .expect("Failed to spawn hub");

    let client = reqwest::Client::new();
    let mcp_url = format!("http://127.0.0.1:{}/mcp", hub_port);
    let auth = format!("Bearer {}", auth_token);

    // Initialize.
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0.1.0" }
        })),
    );
    client
        .post(&mcp_url)
        .header("Authorization", &auth)
        .json(&init_req)
        .send()
        .await
        .expect("initialize");

    // tools/list.
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);
    let resp = client
        .post(&mcp_url)
        .header("Authorization", &auth)
        .json(&list_req)
        .send()
        .await
        .expect("tools/list");

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("parse");

    let tools = body
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .expect("tools array");

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();

    assert!(
        tool_names.contains(&"test_tool"),
        "test_tool not found in tools/list; got: {:?}",
        tool_names
    );

    process.kill().ok();
}

/// Test that tools/call routes correctly to the HTTP backend.
#[tokio::test]
async fn test_streamable_http_backend_tool_call_routes_correctly() {
    let backend = BackendHandle::spawn("test_tool").await;

    let hub_port = find_available_port();
    let admin_port = find_available_port();
    let auth_token = "sse-test-token-3";

    let mock_path = mock_server_bin();
    let mock_str = mock_path.to_str().expect("mock path").to_string();

    let (mut process, _config) =
        spawn_hub_with_http_backend(hub_port, admin_port, auth_token, backend.port, &mock_str)
            .expect("Failed to spawn hub");

    let client = reqwest::Client::new();
    let mcp_url = format!("http://127.0.0.1:{}/mcp", hub_port);
    let auth = format!("Bearer {}", auth_token);

    // Initialize.
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0.1.0" }
        })),
    );
    client
        .post(&mcp_url)
        .header("Authorization", &auth)
        .json(&init_req)
        .send()
        .await
        .expect("initialize");

    // Confirm test_tool is in list.
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);
    let list_resp = client
        .post(&mcp_url)
        .header("Authorization", &auth)
        .json(&list_req)
        .send()
        .await
        .expect("tools/list");
    let list_body: Value = list_resp.json().await.expect("parse list");
    let empty = vec![];
    let tool_names: Vec<&str> = list_body
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .unwrap_or(&empty)
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(
        tool_names.contains(&"test_tool"),
        "test_tool not available; list: {:?}",
        tool_names
    );

    // Call test_tool.
    let call_req = JsonRpcRequest::new(
        RequestId::Number(3),
        "tools/call".to_string(),
        Some(json!({
            "name": "test_tool",
            "arguments": { "input": "hello" }
        })),
    );
    let resp = client
        .post(&mcp_url)
        .header("Authorization", &auth)
        .json(&call_req)
        .send()
        .await
        .expect("tools/call");

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("parse");

    assert!(
        body.get("result").is_some(),
        "expected result in response; got: {:?}",
        body
    );
    assert!(
        body.get("error").is_none(),
        "unexpected error in response: {:?}",
        body
    );

    let content_text = body
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    assert!(
        content_text.contains("test_tool") || content_text.contains("hello"),
        "unexpected content from backend: {}",
        content_text
    );

    process.kill().ok();
}
