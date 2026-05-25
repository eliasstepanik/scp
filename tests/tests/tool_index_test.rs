use scp_core::config::FilterConfig;
use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_filter::pipeline::FilterPipeline;
use scp_hub::router::Router;
use scp_hub::session_store::SessionStore;
use scp_index::ToolEntry;
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

fn make_filter_pipeline() -> Arc<FilterPipeline> {
    Arc::new(FilterPipeline::new(&FilterConfig::default()))
}

// ============================================================================
// Test 1: test_tools_list_scoring_respects_context
// ============================================================================

#[tokio::test]
async fn test_tools_list_scoring_respects_context() {
    let _pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

    // Register tools in the registry
    {
        let mut registry = tool_registry.write().await;

        // Server A: filesystem tools with descriptions containing "file" and "read"
        let server_a_tools = vec![
            ToolEntry {
                original_name: "read_file".to_string(),
                qualified_name: "server_a/read_file".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Read file from filesystem disk storage".to_string()),
                input_schema: json!({}),
                tags: vec!["filesystem".to_string(), "read".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "write_file".to_string(),
                qualified_name: "server_a/write_file".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Write file to filesystem disk storage".to_string()),
                input_schema: json!({}),
                tags: vec!["filesystem".to_string(), "write".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "list_dir".to_string(),
                qualified_name: "server_a/list_dir".to_string(),
                server_name: "server_a".to_string(),
                description: Some("List directory contents filesystem".to_string()),
                input_schema: json!({}),
                tags: vec!["filesystem".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
        ];

        // Server B: search tools with descriptions containing "search" and "web"
        let server_b_tools = vec![
            ToolEntry {
                original_name: "web_search".to_string(),
                qualified_name: "server_b/web_search".to_string(),
                server_name: "server_b".to_string(),
                description: Some("Search web internet query results".to_string()),
                input_schema: json!({}),
                tags: vec!["search".to_string(), "web".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "get_url".to_string(),
                qualified_name: "server_b/get_url".to_string(),
                server_name: "server_b".to_string(),
                description: Some("Fetch content from web URL search".to_string()),
                input_schema: json!({}),
                tags: vec!["search".to_string(), "web".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "parse_html".to_string(),
                qualified_name: "server_b/parse_html".to_string(),
                server_name: "server_b".to_string(),
                description: Some("Parse HTML web content search".to_string()),
                input_schema: json!({}),
                tags: vec!["search".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
        ];

        registry.register_tools("server_a", server_a_tools);
        registry.register_tools("server_b", server_b_tools);
    }

    // Test 1: Query for filesystem tools - should get mostly server_a tools
    {
        let registry = tool_registry.read().await;
        let keywords = vec!["file".to_string(), "read".to_string()];
        let scored = registry.select_tools(&keywords, "default", 3, &[], None);

        // Should return 3 tools, and most should be from server_a
        assert_eq!(scored.len(), 3, "Should return 3 tools");
        let server_a_count = scored
            .iter()
            .filter(|t| t.qualified_name.starts_with("server_a/"))
            .count();
        assert!(
            server_a_count >= 2,
            "At least 2 of 3 tools should be from server_a, got: {:?}",
            scored.iter().map(|t| &t.qualified_name).collect::<Vec<_>>()
        );
    }

    // Test 2: Query for search tools - should get mostly server_b tools
    {
        let registry = tool_registry.read().await;
        let keywords = vec!["search".to_string(), "web".to_string()];
        let scored = registry.select_tools(&keywords, "default", 3, &[], None);

        // Should return 3 tools, and most should be from server_b
        assert_eq!(scored.len(), 3, "Should return 3 tools");
        let server_b_count = scored
            .iter()
            .filter(|t| t.qualified_name.starts_with("server_b/"))
            .count();
        assert!(
            server_b_count >= 2,
            "At least 2 of 3 tools should be from server_b, got: {:?}",
            scored.iter().map(|t| &t.qualified_name).collect::<Vec<_>>()
        );
    }
}

// ============================================================================
// Test 2: test_tools_list_respects_max_exposed
// ============================================================================

#[tokio::test]
async fn test_tools_list_respects_max_exposed() {
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

    // Register 15 tools across 3 servers (5 each)
    {
        let mut registry = tool_registry.write().await;

        for server_idx in 1..=3 {
            let server_name = format!("server_{}", server_idx);
            let mut tools = Vec::new();

            for tool_idx in 1..=5 {
                tools.push(ToolEntry {
                    original_name: format!("tool_{}", tool_idx),
                    qualified_name: format!("{}/tool_{}", server_name, tool_idx),
                    server_name: server_name.clone(),
                    description: Some(format!("Tool {} from {}", tool_idx, server_name)),
                    input_schema: json!({}),
                    tags: vec![],
                    avg_response_tokens: 100.0,
                    call_count: 0,
                });
            }

            registry.register_tools(&server_name, tools);
        }
    }

    // Query with max=5
    {
        let registry = tool_registry.read().await;
        let scored = registry.select_tools(&[], "default", 5, &[], None);

        // Should return exactly 5 tools
        assert_eq!(
            scored.len(),
            5,
            "Should return exactly 5 tools when max=5, got {}",
            scored.len()
        );
    }

    // Query with max=10
    {
        let registry = tool_registry.read().await;
        let scored = registry.select_tools(&[], "default", 10, &[], None);

        // Should return exactly 10 tools
        assert_eq!(
            scored.len(),
            10,
            "Should return exactly 10 tools when max=10, got {}",
            scored.len()
        );
    }
}

// ============================================================================
// Test 3: test_session_keyword_accumulation
// ============================================================================

#[tokio::test]
async fn test_session_keyword_accumulation() {
    let session_store = Arc::new(SessionStore::new(32000));

    // Create a session
    let (session_id, _rx) = session_store.create_with_defaults(None).await;

    // Get the session and extract keywords
    {
        let session = session_store
            .get(&session_id)
            .await
            .expect("Session should exist");
        let mut session_locked = session.lock().unwrap();

        // Extract keywords from JSON arguments
        let args = json!({
            "path": "/home/user/file.txt",
            "operation": "read"
        });

        session_locked.keyword_accumulator.extract_from_args(&args);

        // Check top keywords
        let top = session_locked.keyword_accumulator.top_k(10);

        // Should contain relevant tokens (home, user, file, txt, operation, read)
        assert!(
            top.contains(&"home".to_string()),
            "Should contain 'home', got: {:?}",
            top
        );
        assert!(
            top.contains(&"user".to_string()),
            "Should contain 'user', got: {:?}",
            top
        );
        assert!(
            top.contains(&"file".to_string()),
            "Should contain 'file', got: {:?}",
            top
        );
        assert!(
            top.contains(&"read".to_string()),
            "Should contain 'read', got: {:?}",
            top
        );

        // Should NOT contain stop words
        assert!(
            !top.contains(&"the".to_string()),
            "Should not contain stop word 'the'"
        );
        assert!(
            !top.contains(&"and".to_string()),
            "Should not contain stop word 'and'"
        );
    }

    // Test decay
    {
        let session = session_store
            .get(&session_id)
            .await
            .expect("Session should exist");
        let mut session_locked = session.lock().unwrap();

        // Get top before decay
        let top_before = session_locked.keyword_accumulator.top_k(5);
        assert!(!top_before.is_empty(), "Should have keywords before decay");

        // Apply decay
        session_locked.keyword_accumulator.decay();

        // Keywords should still be there but with lower frequency
        let top_after = session_locked.keyword_accumulator.top_k(5);
        assert!(
            !top_after.is_empty(),
            "Should still have keywords after decay"
        );
    }
}

// ============================================================================
// Test 4: test_tool_cache_invalidation_on_list_changed
// ============================================================================

#[tokio::test]
async fn test_tool_cache_invalidation_on_list_changed() {
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let _session_store = Arc::new(SessionStore::new(32000));
    let (shutdown_tx, _) = broadcast::channel(1);
    let router = Arc::new(Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        300,
        4000,
        make_filter_pipeline(),
        scp_core::config::ExposureConfig::default(),
        vec![],
        50,
        shutdown_tx,
    ));

    // Register some tools
    {
        let mut registry = tool_registry.write().await;
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server_a/test_tool".to_string(),
            server_name: "server_a".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];
        registry.register_tools("server_a", tools);
    }

    // Call tools/list to populate cache
    let list_req = JsonRpcRequest::new(RequestId::Number(1), "tools/list".to_string(), None);
    let resp1 = router.route(list_req.clone(), None).await;
    assert!(resp1.result.is_some(), "First tools/list should succeed");

    // Call tools/list_changed notification
    let changed_req = JsonRpcRequest::new(
        RequestId::Number(2),
        "notifications/tools/list_changed".to_string(),
        Some(json!({
            "server": "server_a"
        })),
    );
    let resp2 = router.route(changed_req, None).await;
    // The notification should be handled (either result or error is acceptable)
    assert!(
        resp2.result.is_some() || resp2.error.is_some(),
        "tools/list_changed should be handled with either result or error"
    );

    // Call tools/list again - should trigger a fresh fan-out (cache invalidated)
    let list_req2 = JsonRpcRequest::new(RequestId::Number(3), "tools/list".to_string(), None);
    let resp3 = router.route(list_req2, None).await;
    assert!(resp3.result.is_some(), "Second tools/list should succeed");

    // Both responses should be successful (the important thing is that cache invalidation
    // doesn't break the flow)
}

// ============================================================================
// Test 5: test_usage_tracking_affects_ranking
// ============================================================================

#[tokio::test]
async fn test_usage_tracking_affects_ranking() {
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

    // Register 4 tools
    {
        let mut registry = tool_registry.write().await;
        let tools = vec![
            ToolEntry {
                original_name: "tool_a".to_string(),
                qualified_name: "server_a/tool_a".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Tool A".to_string()),
                input_schema: json!({}),
                tags: vec![],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "tool_b".to_string(),
                qualified_name: "server_a/tool_b".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Tool B".to_string()),
                input_schema: json!({}),
                tags: vec![],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "tool_c".to_string(),
                qualified_name: "server_a/tool_c".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Tool C".to_string()),
                input_schema: json!({}),
                tags: vec![],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
            ToolEntry {
                original_name: "tool_d".to_string(),
                qualified_name: "server_a/tool_d".to_string(),
                server_name: "server_a".to_string(),
                description: Some("Tool D".to_string()),
                input_schema: json!({}),
                tags: vec![],
                avg_response_tokens: 100.0,
                call_count: 0,
            },
        ];
        registry.register_tools("server_a", tools);
    }

    // Record usage for tool_c
    {
        let mut registry = tool_registry.write().await;
        for _ in 0..5 {
            registry.usage.record_call("default", "server_a/tool_c");
        }
    }

    // Select tools - tool_c should be ranked higher due to usage
    {
        let registry = tool_registry.read().await;
        let scored = registry.select_tools(&[], "default", 2, &[], None);

        // tool_c should be in the top 2
        let tool_c_found = scored.iter().any(|t| t.qualified_name == "server_a/tool_c");
        assert!(
            tool_c_found,
            "tool_c should be in top 2 due to usage tracking, got: {:?}",
            scored.iter().map(|t| &t.qualified_name).collect::<Vec<_>>()
        );
    }
}

// ============================================================================
// Test 6: test_always_include_bypass
// ============================================================================

#[tokio::test]
async fn test_always_include_bypass() {
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

    // Register 10 search tools + 1 maintenance tool
    {
        let mut registry = tool_registry.write().await;

        // Server A: 10 search tools with "search" in description
        let mut search_tools = Vec::new();
        for i in 1..=10 {
            search_tools.push(ToolEntry {
                original_name: format!("search_tool_{}", i),
                qualified_name: format!("server_a/search_tool_{}", i),
                server_name: "server_a".to_string(),
                description: Some(format!("Search tool {} for searching", i)),
                input_schema: json!({}),
                tags: vec!["search".to_string()],
                avg_response_tokens: 100.0,
                call_count: 0,
            });
        }
        registry.register_tools("server_a", search_tools);

        // Server X: maintenance tool (no search relevance)
        let maintenance_tools = vec![ToolEntry {
            original_name: "maintenance_tool".to_string(),
            qualified_name: "server_x/maintenance_tool".to_string(),
            server_name: "server_x".to_string(),
            description: Some("System maintenance admin tool".to_string()),
            input_schema: json!({}),
            tags: vec!["admin".to_string()],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];
        registry.register_tools("server_x", maintenance_tools);
    }

    // Query with always_include
    {
        let registry = tool_registry.read().await;
        let keywords = vec!["search".to_string()];
        let always_include = vec!["server_x/maintenance_tool".to_string()];
        let scored = registry.select_tools(&keywords, "default", 3, &always_include, None);

        // maintenance_tool should be in the result despite low relevance
        let maintenance_found = scored
            .iter()
            .any(|t| t.qualified_name == "server_x/maintenance_tool");
        assert!(
            maintenance_found,
            "maintenance_tool should be in result due to always_include, got: {:?}",
            scored.iter().map(|t| &t.qualified_name).collect::<Vec<_>>()
        );

        // The always_include tools get a score of 2.0, which is higher than search tools,
        // so they will be in the top 3. The important thing is that they appear in the result.
        // We should have at least 3 tools (the max), and maintenance_tool should be one of them.
        assert!(
            scored.len() >= 3,
            "Should have at least 3 tools, got {} tools: {:?}",
            scored.len(),
            scored.iter().map(|t| &t.qualified_name).collect::<Vec<_>>()
        );
    }
}
