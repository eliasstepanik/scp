use crate::error::TransportError;
use scp_core::protocol::IncomingMessage;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};

/// Server-facing HTTP transport (connects to a backend MCP server via Streamable HTTP)
pub struct HttpServerTransport {
    url: String,
    session_id: Option<String>,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    reconnect_attempts: u32,
}

impl HttpServerTransport {
    /// Create a new HTTP server transport
    pub fn new(url: String, headers: HashMap<String, String>) -> Self {
        Self {
            url,
            session_id: None,
            client: reqwest::Client::new(),
            headers,
            reconnect_attempts: 0,
        }
    }

    /// Send a JSON-RPC message to the backend via POST /mcp
    pub async fn send(&mut self, msg: &Value) -> Result<(), TransportError> {
        let json_str = serde_json::to_string(msg)?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().map_err(|_| {
                TransportError::InvalidMessage("Failed to parse content-type header".to_string())
            })?,
        );
        headers.insert(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream".parse().map_err(|_| {
                TransportError::InvalidMessage("Failed to parse accept header".to_string())
            })?,
        );

        // Add custom headers
        for (key, value) in &self.headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).map_err(|_| {
                    TransportError::InvalidMessage(format!("Invalid header name: {}", key))
                })?,
                value.parse().map_err(|_| {
                    TransportError::InvalidMessage(format!("Invalid header value: {}", value))
                })?,
            );
        }

        // Add session ID header if set
        if let Some(session_id) = &self.session_id {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(b"Mcp-Session-Id").map_err(|_| {
                    TransportError::InvalidMessage("Failed to create session ID header".to_string())
                })?,
                session_id.parse().map_err(|_| {
                    TransportError::InvalidMessage("Invalid session ID".to_string())
                })?,
            );
        }

        let response = self
            .client
            .post(format!("{}/mcp", self.url))
            .headers(headers)
            .body(json_str)
            .send()
            .await
            .map_err(|e| TransportError::ProcessError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(TransportError::ProcessError(format!(
                "HTTP error: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        Ok(())
    }

    /// Receive JSON-RPC messages from the backend via GET /mcp SSE stream
    pub async fn receive(&mut self) -> Result<Option<IncomingMessage>, TransportError> {
        loop {
            match self.receive_with_reconnect().await {
                Ok(Some(msg)) => return Ok(Some(msg)),
                Ok(None) => {
                    // Stream ended, attempt reconnect
                    self.reconnect_attempts += 1;
                    if self.reconnect_attempts > 5 {
                        return Err(TransportError::ProcessError(
                            "Max reconnection attempts exceeded".to_string(),
                        ));
                    }

                    let backoff = Duration::from_secs(std::cmp::min(
                        2_u64.pow(self.reconnect_attempts - 1),
                        30,
                    ));
                    warn!("SSE stream disconnected, reconnecting in {:?}", backoff);
                    sleep(backoff).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn receive_with_reconnect(&mut self) -> Result<Option<IncomingMessage>, TransportError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "text/event-stream".parse().map_err(|_| {
                TransportError::InvalidMessage("Failed to parse accept header".to_string())
            })?,
        );

        // Add session ID header if set
        if let Some(session_id) = &self.session_id {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(b"Mcp-Session-Id").map_err(|_| {
                    TransportError::InvalidMessage("Failed to create session ID header".to_string())
                })?,
                session_id.parse().map_err(|_| {
                    TransportError::InvalidMessage("Invalid session ID".to_string())
                })?,
            );
        }

        let response = self
            .client
            .get(format!("{}/mcp", self.url))
            .headers(headers)
            .send()
            .await
            .map_err(|e| TransportError::ProcessError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(TransportError::ProcessError(format!(
                "HTTP error: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Extract session ID from response headers if not already set
        if self.session_id.is_none() {
            if let Some(session_id_header) = response.headers().get("Mcp-Session-Id") {
                if let Ok(session_id_str) = session_id_header.to_str() {
                    self.session_id = Some(session_id_str.to_string());
                    debug!("Established session: {:?}", self.session_id);
                }
            }
        }

        // Parse SSE stream
        let text = response
            .text()
            .await
            .map_err(|e| TransportError::ProcessError(format!("Failed to read response: {}", e)))?;

        // Parse SSE data lines
        for line in text.lines() {
            if line.starts_with("data:") {
                let data = line.strip_prefix("data:").unwrap_or("").trim();
                if !data.is_empty() {
                    let msg: IncomingMessage = serde_json::from_str(data)?;
                    self.reconnect_attempts = 0; // Reset on successful receive
                    return Ok(Some(msg));
                }
            }
        }

        Ok(None)
    }

    /// Close the session via DELETE /mcp
    pub async fn close(&self) -> Result<(), TransportError> {
        let mut headers = reqwest::header::HeaderMap::new();

        // Add session ID header if set
        if let Some(session_id) = &self.session_id {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(b"Mcp-Session-Id").map_err(|_| {
                    TransportError::InvalidMessage("Failed to create session ID header".to_string())
                })?,
                session_id.parse().map_err(|_| {
                    TransportError::InvalidMessage("Invalid session ID".to_string())
                })?,
            );
        }

        let response = self
            .client
            .delete(format!("{}/mcp", self.url))
            .headers(headers)
            .send()
            .await
            .map_err(|e| TransportError::ProcessError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(TransportError::ProcessError(format!(
                "HTTP error: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        debug!("Closed session: {:?}", self.session_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_server_transport_creation() {
        let transport =
            HttpServerTransport::new("http://localhost:8080".to_string(), HashMap::new());
        assert_eq!(transport.url, "http://localhost:8080");
        assert!(transport.session_id.is_none());
    }
}
