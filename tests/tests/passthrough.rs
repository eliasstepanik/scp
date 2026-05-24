use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_tests::HttpTestHub;
use serde_json::json;

/// Test that initialize handshake works over HTTP
#[tokio::test]
async fn test_initialize_handshake() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Send initialize request
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1.0"
            }
        })),
    );

    let response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert_eq!(body.get("id"), Some(&json!(1)));
    assert!(body.get("result").is_some());
    assert!(body.get("error").is_none());

    let result = body.get("result").unwrap();
    assert_eq!(
        result
            .get("serverInfo")
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str()),
        Some("scp")
    );

    hub.kill().ok();
}

/// Test that tools/list passthrough works
#[tokio::test]
async fn test_tools_list_passthrough() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // First, initialize
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1.0"
            }
        })),
    );

    let _init_response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send initialize");

    // Send tools/list request
    let tools_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);

    let response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&tools_req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert_eq!(body.get("id"), Some(&json!(2)));
    assert!(body.get("result").is_some());
    assert!(body.get("error").is_none());

    let result = body.get("result").unwrap();
    assert!(result.get("tools").is_some());

    hub.kill().ok();
}

/// Test that tools/call passthrough works
#[tokio::test]
async fn test_tools_call_passthrough() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // First, initialize
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1.0"
            }
        })),
    );

    let _init_response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send initialize");

    // List tools to confirm scp_info is available
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);

    let list_response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&list_req)
        .send()
        .await
        .expect("Failed to send tools/list");

    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("Failed to parse tools/list response");

    // Verify scp_info is in the tools list
    let tool_names: Vec<String> = list_body
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tool| {
                    tool.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    assert!(
        tool_names.contains(&"scp_info".to_string()),
        "scp_info tool not found in tools/list response. Available tools: {:?}",
        tool_names
    );

    // Send tools/call request for scp_info with empty arguments
    let call_req = JsonRpcRequest::new(
        RequestId::Number(3),
        "tools/call".to_string(),
        Some(json!({
            "name": "scp_info",
            "arguments": {}
        })),
    );

    let response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&call_req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert_eq!(body.get("id"), Some(&json!(3)));
    assert!(body.get("result").is_some(), "Response body: {:?}", body);
    assert!(body.get("error").is_none());

    let result = body.get("result").unwrap();
    assert!(result.get("content").is_some());

    // Verify the content contains scp or version information
    let content = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    assert!(
        content.to_lowercase().contains("scp") || content.to_lowercase().contains("version"),
        "Content does not contain expected scp or version information: {}",
        content
    );

    hub.kill().ok();
}

/// Test that ping is handled by SCP
#[tokio::test]
async fn test_ping_handled_by_scp() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // First, initialize
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1.0"
            }
        })),
    );

    let _init_response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send initialize");

    // Send ping request
    let ping_req = JsonRpcRequest::new(RequestId::Number(3), "ping".to_string(), None);

    let response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&ping_req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert_eq!(body.get("id"), Some(&json!(3)));
    assert!(body.get("result").is_some());
    assert!(body.get("error").is_none());
    assert_eq!(body.get("result").unwrap(), &json!({}));

    hub.kill().ok();
}

/// Test that request IDs are properly remapped
#[tokio::test]
async fn test_id_remapping() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // First, initialize
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "0.1.0"
            }
        })),
    );

    let _init_response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send initialize");

    // Send request with id=100
    let req1 = JsonRpcRequest::new(RequestId::Number(100), "tools/list".to_string(), None);

    let response1 = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&req1)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response1.status(), 200);

    let body1: serde_json::Value = response1.json().await.expect("Failed to parse response");

    assert_eq!(body1.get("id"), Some(&json!(100)));
    assert!(body1.get("result").is_some());

    // Send request with id=200
    let req2 = JsonRpcRequest::new(RequestId::Number(200), "tools/list".to_string(), None);

    let response2 = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&req2)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response2.status(), 200);

    let body2: serde_json::Value = response2.json().await.expect("Failed to parse response");

    assert_eq!(body2.get("id"), Some(&json!(200)));
    assert!(body2.get("result").is_some());

    hub.kill().ok();
}
