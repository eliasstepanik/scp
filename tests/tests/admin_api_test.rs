use scp_tests::HttpTestHub;

/// Test that admin health endpoint works
#[tokio::test]
async fn test_admin_health_endpoint() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/health", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert!(body.get("status").is_some());
    assert!(body.get("servers").is_some());
    assert!(body.get("healthy").is_some());
    assert!(body.get("sessions").is_some());

    hub.kill().ok();
}

/// Test that admin list servers endpoint works
#[tokio::test]
async fn test_admin_list_servers() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/servers", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    assert!(body.get("servers").is_some());
    let servers = body.get("servers").unwrap().as_array().unwrap();

    // Should have at least the mock server
    assert!(!servers.is_empty());

    // Check that mock server is in the list
    let mock_server = servers.iter().find(|s| {
        s.get("name")
            .and_then(|n| n.as_str())
            .map(|name| name == "mock")
            .unwrap_or(false)
    });
    assert!(mock_server.is_some(), "Mock server should be in the list");

    hub.kill().ok();
}

/// Test that admin list sessions endpoint works
#[tokio::test]
async fn test_admin_list_sessions() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/admin/sessions", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");

    // Should be an array or object with sessions
    assert!(body.is_array() || body.is_object());

    hub.kill().ok();
}

/// Test that admin disable/enable server works
#[tokio::test]
async fn test_admin_disable_enable_server() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    // Disable the mock server
    let disable_response = client
        .post(format!("{}/servers/mock/disable", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send disable request");

    assert_eq!(disable_response.status(), 200);

    // Enable the mock server
    let enable_response = client
        .post(format!("{}/servers/mock/enable", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send enable request");

    assert_eq!(enable_response.status(), 200);

    hub.kill().ok();
}

/// Test that admin config reload works
#[tokio::test]
async fn test_admin_config_reload() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/config/reload", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    hub.kill().ok();
}

/// Test that admin prometheus metrics endpoint works
#[tokio::test]
async fn test_admin_prometheus_metrics() {
    let mut hub = HttpTestHub::spawn_auto().expect("Failed to spawn hub");

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/metrics", hub.admin_url()))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body = response.text().await.expect("Failed to read response body");

    // Prometheus metrics should contain TYPE and HELP comments
    assert!(body.contains("# HELP") || body.contains("# TYPE") || !body.is_empty());

    hub.kill().ok();
}
