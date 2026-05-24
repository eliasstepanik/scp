use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_tests::HttpTestHub;
use serde_json::json;

/// Test that unknown method returns JSON-RPC error -32601
#[tokio::test]
async fn test_unknown_method_returns_error() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Send request with unknown method
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "nonexistent/method",
        "params": {}
    });

    let response = client
        .post(&hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    // Should have an error field
    assert!(
        body.get("error").is_some(),
        "Response should contain error field"
    );

    let error = body.get("error").unwrap();
    assert!(error.get("code").is_some());
    assert!(error.get("message").is_some());

    // Error code should be -32601 (method not found)
    let code = error.get("code").and_then(|c| c.as_i64());
    assert_eq!(
        code,
        Some(-32601),
        "Error code should be -32601 (method not found)"
    );

    hub.kill().ok();
}

/// Test that tools/call with unknown tool returns JSON-RPC error
#[tokio::test]
async fn test_tools_call_unknown_tool() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // First initialize
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
        .post(&hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("Failed to send initialize");

    // Send tools/call with unknown tool
    let call_req = JsonRpcRequest::new(
        RequestId::Number(2),
        "tools/call".to_string(),
        Some(json!({
            "name": "no_such_tool",
            "arguments": {}
        })),
    );

    let response = client
        .post(&hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&call_req)
        .send()
        .await
        .expect("Failed to send request");

    // Should return 200 with error in body (not 500)
    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    // Should have an error field
    assert!(
        body.get("error").is_some(),
        "Response should contain error field"
    );
    assert!(body.get("id").is_some());

    hub.kill().ok();
}

/// Test that notifications (no id) return 202 with no response body
#[tokio::test]
async fn test_notification_no_response() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Send a notification (no id field)
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });

    let response = client
        .post(&hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&notif)
        .send()
        .await
        .expect("Failed to send request");

    // Notifications should return 202 Accepted
    assert_eq!(response.status(), 202);

    hub.kill().ok();
}
