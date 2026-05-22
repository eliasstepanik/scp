use crate::error::TransportError;
use scp_core::protocol::IncomingMessage;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Client-facing stdio transport (reads from stdin, writes to stdout)
pub struct StdioClientTransport {
    reader: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl StdioClientTransport {
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    /// Send a JSON-RPC message to stdout
    pub async fn send(&mut self, msg: &Value) -> Result<(), TransportError> {
        let json_str = serde_json::to_string(msg)?;
        self.stdout.write_all(json_str.as_bytes()).await?;
        self.stdout.write_all(b"\n").await?;
        self.stdout.flush().await?;
        Ok(())
    }

    /// Receive a JSON-RPC message from stdin
    pub async fn receive(&mut self) -> Result<Option<IncomingMessage>, TransportError> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line).await? {
                0 => return Ok(None), // EOF
                _ => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        let msg: IncomingMessage = serde_json::from_str(trimmed)?;
                        return Ok(Some(msg));
                    }
                    // Empty line, continue loop
                }
            }
        }
    }
}

impl Default for StdioClientTransport {
    fn default() -> Self {
        Self::new()
    }
}
