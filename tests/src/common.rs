use anyhow::Result;
use scp_core::mcp_types::{CallToolResult, Implementation, InitializeResult, Tool, ToolContent};
use scp_transport::stdio_client::StdioClientTransport;
use serde_json::json;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

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
