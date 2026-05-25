use anyhow::Result;
use scp_core::mcp_types::{CallToolResult, Implementation, InitializeResult, Tool, ToolContent};
use scp_transport::stdio_client::StdioClientTransport;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

/// Send a minimal HTTP GET to `host:port/path` and return true iff the response
/// starts with `HTTP/1.1 200` (i.e. the server is up and the endpoint exists).
/// Uses a raw TCP socket so no async runtime or blocking reqwest feature is needed.
fn http_get_is_ok(host: &str, port: u16, path: &str) -> bool {
    let addr = format!("{}:{}", host, port);
    let mut stream = match TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        Duration::from_millis(500),
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request = format!("GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 15];
    if stream.read_exact(&mut buf).is_err() {
        return false;
    }
    buf.starts_with(b"HTTP/1.1 200") || buf.starts_with(b"HTTP/1.0 200")
}

/// Kill all processes matching the given executable name (best-effort, ignores errors).
/// On Windows this uses `taskkill /F /IM <name>` to forcibly terminate all instances.
fn kill_processes_by_name(exe_name: &str) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", exe_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(not(target_os = "windows"))]
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(exe_name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Mock MCP server for testing
pub struct MockMcpServer {
    process: Child,
}

impl MockMcpServer {
    /// Spawn a mock MCP server
    pub fn spawn() -> Result<Self> {
        // For Phase 0, we'll create a simple in-process mock that communicates via stdio
        // This is a placeholder that will be replaced with a real binary in later phases

        let process = Command::new("cargo")
            .args(["run", "--bin", "mock-mcp-server"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Give the process time to start
        thread::sleep(Duration::from_millis(100));

        Ok(Self { process })
    }

    /// Kill the mock server
    pub fn kill(&mut self) -> Result<()> {
        self.process.kill()?;
        Ok(())
    }
}

impl Drop for MockMcpServer {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

/// Helper to spawn SCP with a mock server
pub async fn spawn_scp_with_mock() -> Result<StdioClientTransport> {
    // This will be implemented in Phase 0.11
    // For now, return a placeholder
    Ok(StdioClientTransport::new())
}

/// HTTP-based test helper for spawning scp-hub with HTTP listener
pub struct HttpTestHub {
    pub process: Child,
    pub config_file: tempfile::NamedTempFile,
    pub port: u16,
    pub admin_port: u16,
    pub auth_token: String,
}

impl HttpTestHub {
    /// Get the path to the scp-hub binary
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

    /// Get the path to the mock-mcp-server binary
    fn mock_server_bin() -> PathBuf {
        // Use the CARGO_BIN_EXE_mock-mcp-server environment variable set by cargo during integration tests
        // This is the most reliable way to get the compiled binary path
        std::env::var("CARGO_BIN_EXE_mock-mcp-server")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Fallback to manual path construction if env var is not set
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

    /// Create a temporary TOML config file for testing
    fn create_test_config(
        mock_server_path: &str,
        port: u16,
        admin_port: u16,
        auth_token: &str,
    ) -> Result<tempfile::NamedTempFile> {
        let config_content = format!(
            r#"config_version = 1

[hub]
listen_address = "127.0.0.1"
listen_port = {}
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
token = "{}"
token_budget_per_request = 4000
rate_limit_per_minute = 100

[hub.auth.profiles.limited]
token = "limited-token"
token_budget_per_request = 2000
rate_limit_per_minute = 2

[admin]
port = {}

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
command = "{}"
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
"#,
            port,
            auth_token,
            admin_port,
            mock_server_path.replace("\\", "\\\\")
        );

        let mut temp_file = tempfile::NamedTempFile::new()?;
        temp_file.write_all(config_content.as_bytes())?;
        temp_file.flush()?;
        Ok(temp_file)
    }

    /// Find an available port
    fn find_available_port() -> u16 {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to port 0");
        let addr = listener.local_addr().expect("Failed to get local address");
        addr.port()
    }

    /// Spawn a new HTTP test hub with automatic port allocation
    pub fn spawn_auto() -> Result<Self> {
        let port = Self::find_available_port();
        let admin_port = Self::find_available_port();
        let auth_token = "test-token";
        Self::spawn(port, admin_port, auth_token)
    }

    /// Spawn a new HTTP test hub with specific ports
    pub fn spawn(port: u16, admin_port: u16, auth_token: &str) -> Result<Self> {
        let mock_server_path = Self::mock_server_bin();
        let mock_server_str = mock_server_path
            .to_str()
            .expect("Failed to convert mock server path to string")
            .to_string();

        let config_file = Self::create_test_config(&mock_server_str, port, admin_port, auth_token)?;
        let config_path = config_file.path().to_path_buf();

        let process = Command::new(Self::scp_hub_bin())
            .arg("--config")
            .arg(&config_path)
            .arg("--log-level")
            .arg("error")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Initial sleep to allow hub and mock server to start
        thread::sleep(Duration::from_millis(2000));

        // Poll for hub to be ready: both the admin port AND the MCP /health endpoint
        // must be responsive before the test proceeds. Checking only admin TCP is not
        // sufficient because the admin listener can be ready before the axum HTTP
        // listener on hub.port has finished binding and registering routes.
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(15);

        // Step 1: wait for the admin TCP port to accept connections.
        loop {
            if start.elapsed() > timeout {
                eprintln!("Timeout waiting for admin port to be ready");
                break;
            }

            match std::net::TcpStream::connect(format!("127.0.0.1:{}", admin_port)) {
                Ok(_) => break,
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }

        // Step 2: wait for the MCP /health endpoint to return HTTP 200.
        // This guarantees that axum has fully started and the /mcp route is registered.
        // We use a raw TCP request to avoid needing the reqwest blocking feature.
        loop {
            if start.elapsed() > timeout {
                eprintln!("Timeout waiting for MCP health endpoint to be ready");
                break;
            }

            if http_get_is_ok("127.0.0.1", port, "/health") {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }

        Ok(Self {
            process,
            config_file,
            port,
            admin_port,
            auth_token: auth_token.to_string(),
        })
    }

    /// Get the base URL for MCP requests
    pub fn mcp_url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    /// Get the base URL for admin requests
    pub fn admin_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.admin_port)
    }

    /// Get the authorization header value
    pub fn auth_header(&self) -> String {
        format!("Bearer {}", self.auth_token)
    }

    /// Kill the hub process and any orphaned mock-mcp-server children.
    /// Also waits briefly so Windows releases file handles before the next test rebuilds.
    pub fn kill(&mut self) -> Result<()> {
        let _ = self.process.kill();
        // Kill orphaned stdio children spawned by the hub (Windows keeps them alive
        // after the parent is killed, which locks the exe and breaks the next build).
        kill_processes_by_name("mock-mcp-server.exe");
        // Give Windows a moment to release file handles.
        thread::sleep(Duration::from_millis(300));
        Ok(())
    }
}

impl Drop for HttpTestHub {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

/// Create a mock initialize response
pub fn mock_initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: "2025-03-26".to_string(),
        capabilities: Default::default(),
        server_info: Implementation {
            name: "mock-server".to_string(),
            version: "0.1.0".to_string(),
        },
    }
}

/// Create mock tools for testing
pub fn mock_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "echo".to_string(),
            description: Some("Echoes arguments back".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            }),
        },
        Tool {
            name: "add".to_string(),
            description: Some("Adds two numbers".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                }
            }),
        },
    ]
}

/// Create a mock tool call result
pub fn mock_tool_result(content: &str) -> CallToolResult {
    CallToolResult {
        content: vec![ToolContent::Text {
            text: content.to_string(),
        }],
        is_error: None,
    }
}
