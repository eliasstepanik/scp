use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_tests::HttpTestHub;
use serde_json::{json, Value};
use std::time::Duration;

fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build reqwest client")
}

/// Helper: initialize the hub session and return the client + mcp_url + auth header.
async fn init_hub(hub: &HttpTestHub) -> reqwest::Client {
    let client = test_client();
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "stdio-test-client", "version": "0.1.0" }
        })),
    );
    client
        .post(hub.mcp_url())
        .header("Authorization", hub.auth_header())
        .json(&init_req)
        .send()
        .await
        .expect("initialize failed");
    client
}

/// Test that a stdio backend's tool appears in the hub's tools/list response,
/// namespaced as `mock/echo`.
#[tokio::test]
async fn test_stdio_backend_tools_appear_in_list() {
    let hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");
    let client = init_hub(&hub).await;

    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);
    let resp = client
        .post(hub.mcp_url())
        .header("Authorization", hub.auth_header())
        .json(&list_req)
        .send()
        .await
        .expect("tools/list request failed");

    assert_eq!(resp.status(), 200, "tools/list should return HTTP 200");

    let body: Value = resp.json().await.expect("Failed to parse tools/list body");

    let tools = body
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .expect("result.tools should be an array");

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();

    assert!(
        tool_names.contains(&"mock/echo"),
        "mock/echo not found in tools/list; got: {:?}",
        tool_names
    );
}

/// Test that tools/call routes correctly through the stdio fanout pathway.
/// Calls `mock/echo` and verifies the response comes back from the stdio backend.
#[tokio::test]
async fn test_stdio_backend_tool_call_routes_correctly() {
    let hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");
    let client = init_hub(&hub).await;

    // Confirm mock/echo is reachable before calling it.
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);
    let list_resp = client
        .post(hub.mcp_url())
        .header("Authorization", hub.auth_header())
        .json(&list_req)
        .send()
        .await
        .expect("tools/list request failed");
    let list_body: Value = list_resp.json().await.expect("parse tools/list");
    let tool_names: Vec<&str> = list_body
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        tool_names.contains(&"mock/echo"),
        "mock/echo not available for call; list: {:?}",
        tool_names
    );

    // Call mock/echo.
    let call_req = JsonRpcRequest::new(
        RequestId::Number(3),
        "tools/call".to_string(),
        Some(json!({
            "name": "mock/echo",
            "arguments": { "message": "hello-stdio" }
        })),
    );
    let resp = client
        .post(hub.mcp_url())
        .header("Authorization", hub.auth_header())
        .json(&call_req)
        .send()
        .await
        .expect("tools/call request failed");

    assert_eq!(resp.status(), 200, "tools/call should return HTTP 200");

    let body: Value = resp.json().await.expect("Failed to parse tools/call body");

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

    // The mock server echoes the raw params JSON, which will contain "hello-stdio".
    assert!(
        !content_text.is_empty(),
        "stdio backend returned empty content"
    );
    assert!(
        content_text.contains("hello-stdio") || content_text.contains("message"),
        "stdio backend echo did not contain expected payload; got: {:?}",
        content_text
    );
}
