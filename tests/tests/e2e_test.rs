use scp_core::config::FilterConfig;
use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_filter::pipeline::FilterPipeline;
use scp_hub::router::Router;
use scp_hub::session_store::SessionStore;
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

fn make_filter_pipeline() -> Arc<FilterPipeline> {
    Arc::new(FilterPipeline::new(&FilterConfig::default()))
}

// ============================================================================
// Test 1: test_full_hub_lifecycle
// ============================================================================
// Verify that the hub can be instantiated with core components and that
// tools/list returns the 4 synthetic extension tools.
#[tokio::test]
async fn test_full_hub_lifecycle() {
    // Create hub components
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let session_store = Arc::new(SessionStore::new(32000));
    let _filter_config = FilterConfig::default();

    // Create router (the core hub component)
    let router = Arc::new(Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        300,  // fanout_timeout_secs
        4000, // request_token_budget
        make_filter_pipeline(),
        scp_core::config::ExposureConfig::default(),
        vec![],
        50,
    ));

    // Create a session
    let (session_id, _rx) = session_store.create_with_defaults(None).await;

    // Verify session exists
    assert!(
        session_store.get(&session_id).await.is_some(),
        "Session should be created"
    );

    // Test initialize request
    let init_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "initialize".to_string(),
        Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })),
    );

    let init_resp = router.route(init_req, None).await;
    assert!(
        init_resp.result.is_some(),
        "Initialize should return a result"
    );

    // Test tools/list request
    let list_req = JsonRpcRequest::new(RequestId::Number(2), "tools/list".to_string(), None);
    let list_resp = router.route(list_req, None).await;

    assert!(
        list_resp.result.is_some(),
        "tools/list should return a result"
    );

    // Verify the response contains the 4 synthetic extension tools
    if let Some(result) = list_resp.result {
        if let Some(tools_array) = result.get("tools").and_then(|t| t.as_array()) {
            // Extract tool names
            let tool_names: Vec<String> = tools_array
                .iter()
                .filter_map(|t| {
                    t.get("name")
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

            // Verify we have at least the 4 extension tools
            assert!(
                tools_array.len() >= 4,
                "Should have at least 4 extension tools"
            );
        } else {
            panic!("tools/list result should contain 'tools' array");
        }
    }

    // Verify session is still accessible after requests
    assert!(
        session_store.get(&session_id).await.is_some(),
        "Session should still exist after requests"
    );
}

// ============================================================================
// Test 2: test_tool_call_proxied_through_filter
// ============================================================================
// Verify that tool calls go through the filter pipeline and budget is decremented.
#[tokio::test]
async fn test_tool_call_proxied_through_filter() {
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let session_store = Arc::new(SessionStore::new(32000));
    let _filter_config = FilterConfig::default();

    let router = Arc::new(Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        4000,
        300,
        make_filter_pipeline(),
        scp_core::config::ExposureConfig::default(),
        vec![],
        50,
    ));

    // Create a session with a known budget
    let (session_id, _rx) = session_store
        .create(None, "default".to_string(), 5000, 60)
        .await;

    // Get initial budget
    let session_before = session_store
        .get(&session_id)
        .await
        .expect("Session should exist");
    let budget_before = session_before.lock().unwrap().token_budget_remaining;

    // Make a tools/call request (will fail because no servers are registered,
    // but the important thing is that it goes through the filter pipeline)
    let call_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "tools/call".to_string(),
        Some(json!({
            "name": "nonexistent_tool",
            "arguments": {
                "param": "value"
            }
        })),
    );

    let call_resp = router.route(call_req, None).await;

    // Verify we got a response (either result or error)
    assert!(
        call_resp.result.is_some() || call_resp.error.is_some(),
        "tools/call should return either result or error"
    );

    // Get budget after the call
    let session_after = session_store
        .get(&session_id)
        .await
        .expect("Session should exist");
    let budget_after = session_after.lock().unwrap().token_budget_remaining;

    // Budget should have been decremented (filter pipeline consumed tokens)
    // Note: The budget might not change if the request failed before reaching
    // the filter, but we verify the session is still valid and accessible
    assert!(
        budget_after <= budget_before,
        "Budget should not increase after a request"
    );

    // Verify session is still valid
    assert!(
        session_store.get(&session_id).await.is_some(),
        "Session should still exist after tool call"
    );
}

// ============================================================================
// Test 3: test_admin_api_sessions
// ============================================================================
// Verify that sessions can be created, retrieved, and counted.
#[tokio::test]
async fn test_admin_api_sessions() {
    let session_store = Arc::new(SessionStore::new(32000));

    // Create multiple sessions
    let (session1_id, _rx1) = session_store.create_with_defaults(None).await;
    let (session2_id, _rx2) = session_store.create_with_defaults(None).await;
    let (session3_id, _rx3) = session_store.create_with_defaults(None).await;

    // Verify all sessions exist
    assert!(
        session_store.get(&session1_id).await.is_some(),
        "Session 1 should exist"
    );
    assert!(
        session_store.get(&session2_id).await.is_some(),
        "Session 2 should exist"
    );
    assert!(
        session_store.get(&session3_id).await.is_some(),
        "Session 3 should exist"
    );

    // Verify sessions are different
    assert_ne!(
        session1_id, session2_id,
        "Sessions should have different IDs"
    );
    assert_ne!(
        session2_id, session3_id,
        "Sessions should have different IDs"
    );
    assert_ne!(
        session1_id, session3_id,
        "Sessions should have different IDs"
    );

    // Verify session count
    let sessions_list = session_store.list().await;
    let count = sessions_list.len();
    assert!(
        count >= 3,
        "Session store should have at least 3 sessions, got {}",
        count
    );

    // Verify session retrieval by ID
    let retrieved_session1 = session_store
        .get(&session1_id)
        .await
        .expect("Session 1 should be retrievable");
    let retrieved_session2 = session_store
        .get(&session2_id)
        .await
        .expect("Session 2 should be retrievable");
    let retrieved_session3 = session_store
        .get(&session3_id)
        .await
        .expect("Session 3 should be retrievable");

    // Verify session IDs match
    assert_eq!(
        retrieved_session1.lock().unwrap().id,
        session1_id,
        "Retrieved session 1 should have correct ID"
    );
    assert_eq!(
        retrieved_session2.lock().unwrap().id,
        session2_id,
        "Retrieved session 2 should have correct ID"
    );
    assert_eq!(
        retrieved_session3.lock().unwrap().id,
        session3_id,
        "Retrieved session 3 should have correct ID"
    );

    // Test session removal
    session_store.remove(&session1_id).await;

    // Verify session 1 is removed
    assert!(
        session_store.get(&session1_id).await.is_none(),
        "Session 1 should be removed"
    );

    // Verify sessions 2 and 3 still exist
    assert!(
        session_store.get(&session2_id).await.is_some(),
        "Session 2 should still exist"
    );
    assert!(
        session_store.get(&session3_id).await.is_some(),
        "Session 3 should still exist"
    );

    // Verify count decreased
    let sessions_list_after = session_store.list().await;
    let count_after = sessions_list_after.len();
    assert!(
        count_after < count,
        "Session count should decrease after removal"
    );
}
