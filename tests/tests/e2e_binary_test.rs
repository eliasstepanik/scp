use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_tests::HttpTestHub;
use serde_json::json;
use std::time::{Duration, Instant};

/// Test that hub starts and serves HTTP
#[tokio::test]
async fn test_hub_starts_and_serves_http() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Poll the health endpoint until ready (max 5s)
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    let mut ready = false;

    while start.elapsed() < timeout {
        match client
            .get(format!("{}/health", hub.admin_url()))
            .send()
            .await
        {
            Ok(response) if response.status() == 200 => {
                ready = true;
                break;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    assert!(ready, "Hub should be ready within 5 seconds");

    hub.kill().ok();
}

/// Test that hub MCP initialize works
#[tokio::test]
async fn test_hub_mcp_initialize() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

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

    assert!(body.get("result").is_some());
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

/// Test that hub MCP tools/list returns extension tools
#[tokio::test]
async fn test_hub_mcp_tools_list() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Initialize first
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

    // List tools
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);

    let response = client
        .post(hub.mcp_url())
        .header("Authorization", &hub.auth_header())
        .json(&list_req)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert!(body.get("result").is_some());
    let result = body.get("result").unwrap();
    let tools = result.get("tools").and_then(|t| t.as_array()).unwrap();

    // Extract tool names
    let tool_names: Vec<String> = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Verify all 4 extension tools are present
    assert!(
        tool_names.contains(&"scp_get_more".to_string()),
        "scp_get_more should be in tools list"
    );
    assert!(
        tool_names.contains(&"scp_info".to_string()),
        "scp_info should be in tools list"
    );
    assert!(
        tool_names.contains(&"scp_budget".to_string()),
        "scp_budget should be in tools list"
    );
    assert!(
        tool_names.contains(&"scp_budget_reset".to_string()),
        "scp_budget_reset should be in tools list"
    );

    hub.kill().ok();
}

/// Test that hub extension tool scp_info works
#[tokio::test]
async fn test_hub_extension_tool_scp_info() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Initialize first
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

    // Call scp_info tool
    let call_req = JsonRpcRequest::new(
        RequestId::Number(2),
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

    assert!(body.get("result").is_some());
    let result = body.get("result").unwrap();

    // Should have content field
    assert!(result.get("content").is_some());

    hub.kill().ok();
}
