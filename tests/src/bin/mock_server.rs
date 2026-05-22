use scp_core::mcp_types::{CallToolResult, InitializeResult, ListToolsResult, Tool, ToolContent};
use scp_core::protocol::{
    IncomingMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = stdin.lock();
    let mut lines = reader.lines();

    loop {
        match lines.next() {
            Some(Ok(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match serde_json::from_str::<IncomingMessage>(trimmed) {
                    Ok(msg) => {
                        match msg {
                            IncomingMessage::Request(req) => {
                                handle_request(&mut stdout, &req);
                            }
                            IncomingMessage::Notification(notif) => {
                                handle_notification(&notif);
                            }
                            IncomingMessage::Response(_) => {
                                // Ignore responses
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to parse JSON: {}", e);
                    }
                }
            }
            Some(Err(e)) => {
                eprintln!("Error reading line: {}", e);
                break;
            }
            None => {
                // EOF
                break;
            }
        }
    }
}

fn handle_request(stdout: &mut std::io::Stdout, req: &JsonRpcRequest) {
    let response = match req.method.as_str() {
        "initialize" => {
            let result = InitializeResult {
                protocol_version: "2025-03-26".to_string(),
                capabilities: Default::default(),
                server_info: scp_core::mcp_types::Implementation {
                    name: "mock-server".to_string(),
                    version: "0.1.0".to_string(),
                },
            };
            JsonRpcResponse::success(
                req.id.clone().unwrap_or(RequestId::Null),
                serde_json::to_value(&result).unwrap(),
            )
        }
        "tools/list" => {
            let tools = vec![Tool {
                name: "echo".to_string(),
                description: Some("Echoes arguments back".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    }
                }),
            }];
            let result = ListToolsResult { tools };
            JsonRpcResponse::success(
                req.id.clone().unwrap_or(RequestId::Null),
                serde_json::to_value(&result).unwrap(),
            )
        }
        "tools/call" => {
            let args = req.params.clone().unwrap_or(Value::Object(Default::default()));
            let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            let result = CallToolResult {
                content: vec![ToolContent::Text { text: args_str }],
                is_error: None,
            };
            JsonRpcResponse::success(
                req.id.clone().unwrap_or(RequestId::Null),
                serde_json::to_value(&result).unwrap(),
            )
        }
        "ping" => JsonRpcResponse::success(
            req.id.clone().unwrap_or(RequestId::Null),
            json!({}),
        ),
        _ => {
            let error = scp_core::protocol::JsonRpcError::new(
                scp_core::protocol::JsonRpcError::METHOD_NOT_FOUND,
                format!("Unknown method: {}", req.method),
            );
            JsonRpcResponse::error(req.id.clone().unwrap_or(RequestId::Null), error)
        }
    };

    if let Ok(json_str) = serde_json::to_string(&response) {
        let _ = writeln!(stdout, "{}", json_str);
        let _ = stdout.flush();
    }
}

fn handle_notification(notif: &JsonRpcNotification) {
    match notif.method.as_str() {
        "notifications/initialized" => {
            // No response needed
        }
        _ => {
            // Ignore unknown notifications
        }
    }
}
