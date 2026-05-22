use anyhow::Result;
use scp_core::id_map::IdMap;
use scp_core::mcp_types::{
    ClientCapabilities, Implementation, InitializeParams, InitializeResult, ServerCapabilities,
};
use scp_core::protocol::{
    IncomingMessage, JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
};
use scp_transport::stdio_client::StdioClientTransport;
use scp_transport::stdio_server::StdioServerTransport;
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

#[allow(dead_code)]
const PROTOCOL_VERSION: &str = "2025-03-26";

/// Initialize the backend server
#[allow(dead_code)]
pub async fn initialize_backend(
    transport: &mut StdioServerTransport,
    _client_params: &InitializeParams,
) -> Result<InitializeResult> {
    info!("Initializing backend server");

    // Send initialize request to backend
    let init_request = JsonRpcRequest::new(
        RequestId::String("scp-init-0001".to_string()),
        "initialize".to_string(),
        Some(serde_json::to_value(InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "scp".to_string(),
                version: "0.1.0".to_string(),
            },
        })?),
    );

    transport
        .send(&serde_json::to_value(&init_request)?)
        .await?;
    debug!("Sent initialize request to backend");

    // Receive initialize response
    let response = transport.receive().await?;
    let init_result = match response {
        Some(IncomingMessage::Response(resp)) => {
            if let Some(result) = resp.result {
                serde_json::from_value::<InitializeResult>(result)?
            } else {
                anyhow::bail!("Backend returned error during initialize: {:?}", resp.error);
            }
        }
        _ => anyhow::bail!("Expected initialize response from backend"),
    };

    debug!("Received initialize response from backend");

    // Send initialized notification
    let initialized_notif = JsonRpcNotification::new("notifications/initialized".to_string(), None);
    transport
        .send(&serde_json::to_value(&initialized_notif)?)
        .await?;
    debug!("Sent initialized notification to backend");

    Ok(init_result)
}

/// Handle client initialize request
#[allow(dead_code)]
pub async fn handle_client_initialize(
    client: &mut StdioClientTransport,
    backend_caps: &ServerCapabilities,
) -> Result<(RequestId, InitializeParams)> {
    info!("Waiting for client initialize request");

    // Read from client until we get an initialize request
    loop {
        match client.receive().await? {
            Some(IncomingMessage::Request(req)) if req.method == "initialize" => {
                let client_id = req
                    .id
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Initialize request missing id"))?;

                let params: InitializeParams = serde_json::from_value(
                    req.params.unwrap_or(Value::Object(Default::default())),
                )?;

                debug!(
                    "Received initialize request from client with id: {}",
                    client_id
                );

                // Send initialize response with backend capabilities
                let init_result = InitializeResult {
                    protocol_version: PROTOCOL_VERSION.to_string(),
                    capabilities: backend_caps.clone(),
                    server_info: Implementation {
                        name: "scp".to_string(),
                        version: "0.1.0".to_string(),
                    },
                };

                let response = JsonRpcResponse::success(
                    client_id.clone(),
                    serde_json::to_value(&init_result)?,
                );

                client.send(&serde_json::to_value(&response)?).await?;
                debug!("Sent initialize response to client");

                // Wait for initialized notification
                match client.receive().await? {
                    Some(IncomingMessage::Notification(notif))
                        if notif.method == "notifications/initialized" =>
                    {
                        debug!("Received initialized notification from client");
                    }
                    _ => {
                        warn!("Expected initialized notification from client");
                    }
                }

                return Ok((client_id, params));
            }
            Some(IncomingMessage::Request(req)) => {
                warn!(
                    "Received non-initialize request before initialize: {}",
                    req.method
                );
                let error =
                    JsonRpcError::new(JsonRpcError::INVALID_REQUEST, "Must send initialize first");
                let response = JsonRpcResponse::error(req.id.unwrap_or(RequestId::Null), error);
                client.send(&serde_json::to_value(&response)?).await?;
            }
            Some(msg) => {
                warn!("Received non-request message before initialize: {:?}", msg);
            }
            None => {
                anyhow::bail!("Client disconnected before initialize");
            }
        }
    }
}

/// Main proxy loop
#[allow(dead_code)]
pub async fn run_proxy(
    client: &mut StdioClientTransport,
    backend: &mut StdioServerTransport,
    id_map: &mut IdMap,
) -> Result<()> {
    info!("Starting proxy loop");

    loop {
        tokio::select! {
            // Receive from client
            client_msg = client.receive() => {
                match client_msg? {
                    Some(IncomingMessage::Request(mut req)) => {
                        // Handle ping directly
                        if req.method == "ping" {
                            debug!("Handling ping directly");
                            let response = JsonRpcResponse::success(
                                req.id.clone().unwrap_or(RequestId::Null),
                                json!({}),
                            );
                            client.send(&serde_json::to_value(&response)?).await?;
                            continue;
                        }

                        // Generate internal ID and forward to backend
                        let client_id = req.id.clone().ok_or_else(|| {
                            anyhow::anyhow!("Request missing id")
                        })?;

                        let internal_id = id_map.generate(client_id.clone());
                        req.id = Some(RequestId::String(internal_id.to_string()));

                        debug!("Forwarding request from client {} to backend as {}", client_id, internal_id);
                        backend.send(&serde_json::to_value(&req)?).await?;
                    }
                    Some(IncomingMessage::Notification(notif)) => {
                        // Forward notification to backend as-is
                        debug!("Forwarding notification to backend: {}", notif.method);
                        backend.send(&serde_json::to_value(&notif)?).await?;
                    }
                    Some(IncomingMessage::Response(_)) => {
                        warn!("Received unexpected response from client");
                    }
                    None => {
                        info!("Client disconnected");
                        return Ok(());
                    }
                }
            }

            // Receive from backend
            backend_msg = backend.receive() => {
                match backend_msg? {
                    Some(IncomingMessage::Response(mut resp)) => {
                        // Look up client ID and forward
                        if let Some(RequestId::String(internal_id_str)) = &resp.id {
                            let internal_id = scp_core::id_map::InternalId(internal_id_str.clone());
                            if let Some(client_id) = id_map.remove(&internal_id) {
                                resp.id = Some(client_id.clone());
                                debug!("Forwarding response from backend {} to client {}", internal_id, client_id);
                                client.send(&serde_json::to_value(&resp)?).await?;
                            } else {
                                warn!("Received response for unknown internal ID: {}", internal_id);
                            }
                        } else {
                            warn!("Response missing id");
                        }
                    }
                    Some(IncomingMessage::Notification(notif)) => {
                        // Forward notification to client as-is
                        debug!("Forwarding notification from backend to client: {}", notif.method);
                        client.send(&serde_json::to_value(&notif)?).await?;
                    }
                    Some(IncomingMessage::Request(_)) => {
                        warn!("Received unexpected request from backend");
                    }
                    None => {
                        error!("Backend disconnected");
                        return Err(anyhow::anyhow!("Backend disconnected"));
                    }
                }
            }
        }
    }
}
