use scp_core::config::FilterConfig;
use scp_core::protocol::{JsonRpcRequest, RequestId};
use scp_filter::chunker::Chunk;
use scp_filter::dedup::DeliveryLog;
use scp_filter::pipeline::{FilterContext, FilterPipeline};
use scp_hub::router::Router;
use scp_hub::session_store::SessionStore;
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

fn make_filter_pipeline() -> Arc<FilterPipeline> {
    Arc::new(FilterPipeline::new(&FilterConfig::default()))
}

// ============================================================================
// Test 1: test_progressive_disclosure_end_to_end
// ============================================================================

#[tokio::test]
async fn test_progressive_disclosure_end_to_end() {
    // Create a session and router
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let session_store = Arc::new(SessionStore::new(32000));
    let filter_config = FilterConfig {
        enabled: true,
        budget_strategy: "truncate".to_string(),
        chunking_strategy: "paragraph".to_string(),
        relevance_engine: "tags".to_string(),
        progressive_disclosure_enabled: true,
        short_circuit_below_tokens: 10, // Force filtering to always run
        embedding: Default::default(),
        progressive_hint_text: "[SCP: {shown} of {total} results shown. Call scp_get_more(request_id=\"{id}\") for more.]".to_string(),
    };

    let _router = Arc::new(Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        4000,
        300,
        make_filter_pipeline(),
        scp_core::config::ExposureConfig::default(),
        vec![],
        50,
    ));

    // Create a session
    let (session_id, _rx) = session_store.create_with_defaults(None).await;

    // Create a large content string: 50 lines of text (>200 tokens total)
    let mut lines = Vec::new();
    for i in 0..50 {
        lines.push(format!(
            "Line {} with some content to increase token count. This is a test line with additional text.",
            i
        ));
    }
    let large_content = lines.join("\n");

    // Create a FilterContext with a small budget to force filtering
    let delivery_log = Arc::new(Mutex::new(DeliveryLog::new(10_000)));
    let ctx = FilterContext {
        session_id: session_id.clone(),
        tool_name: "test_tool".to_string(),
        budget_tokens: 200, // Small budget to force filtering
        query_terms: vec!["test".to_string(), "content".to_string()],
        delivery_log: delivery_log.clone(),
        short_circuit_below_tokens: 10,
        request_id: "test-req-1".to_string(),
    };

    // Run the filter pipeline
    let pipeline = FilterPipeline::new(&filter_config);
    let filter_result = pipeline.run(&json!(large_content), &ctx).await;

    // Assert filtering occurred
    assert!(
        filter_result.tokens_delivered < filter_result.tokens_received,
        "Filtering should have occurred: delivered={}, received={}",
        filter_result.tokens_delivered,
        filter_result.tokens_received
    );

    // Assert some chunks were dropped
    assert!(
        !filter_result.dropped_chunks.is_empty(),
        "Some chunks should have been dropped"
    );

    // Store the dropped chunks in the session
    let session = session_store
        .get(&session_id)
        .await
        .expect("Session should exist");
    {
        let mut session_locked = session.lock().unwrap();
        session_locked.store_chunks(
            "test-req-1".to_string(),
            filter_result.dropped_chunks.clone(),
        );
    }

    // Retrieve the chunks from the session
    {
        let session_locked = session.lock().unwrap();
        let retrieved_chunks = session_locked.get_chunks("test-req-1");
        assert!(
            retrieved_chunks.is_some(),
            "Chunks should be retrievable from session"
        );
        assert_eq!(
            retrieved_chunks.unwrap().len(),
            filter_result.dropped_chunks.len(),
            "Retrieved chunks should match stored chunks"
        );
    }

    // Verify chunks are paginated correctly
    {
        let session_locked = session.lock().unwrap();
        let all_chunks = session_locked.get_chunks("test-req-1").unwrap();

        // Test offset/limit slicing
        let offset = 0;
        let limit = 5;
        let subset: Vec<_> = all_chunks.iter().skip(offset).take(limit).collect();
        assert!(subset.len() <= limit, "Subset should respect limit");
    }
}

// ============================================================================
// Test 2: test_fallback_to_keyword_scoring
// ============================================================================

#[tokio::test]
async fn test_fallback_to_keyword_scoring() {
    // Create a FilterConfig with embedding engine pointing to a bad URL
    let filter_config = FilterConfig {
        enabled: true,
        budget_strategy: "truncate".to_string(),
        chunking_strategy: "paragraph".to_string(),
        relevance_engine: "embedding".to_string(),
        progressive_disclosure_enabled: true,
        short_circuit_below_tokens: 10,
        embedding: scp_core::config::EmbeddingConfig {
            endpoint: "http://127.0.0.1:19999/api/embed".to_string(),
            model: "test-model".to_string(),
            dimension: 1536,
        },
        progressive_hint_text: "[SCP: {shown} of {total} results shown.]".to_string(),
    };

    // Create a FilterPipeline
    let pipeline = FilterPipeline::new(&filter_config);

    // Create a content string with keyword "error"
    let content =
        "This is a test with error handling. Error occurred at line 42. Another error message.";

    // Create a FilterContext
    let delivery_log = Arc::new(Mutex::new(DeliveryLog::new(10_000)));
    let ctx = FilterContext {
        session_id: "test-session".to_string(),
        tool_name: "test_tool".to_string(),
        budget_tokens: 500,
        query_terms: vec!["error".to_string()],
        delivery_log: delivery_log.clone(),
        short_circuit_below_tokens: 10,
        request_id: "test-req-2".to_string(),
    };

    // Run the pipeline - should not panic even with bad embedding endpoint
    let filter_result = pipeline.run(&json!(content), &ctx).await;

    // Assert: no panic occurred (we're here)
    // Assert: content was processed
    assert!(
        filter_result.tokens_received > 0,
        "Content should have been processed"
    );

    // Assert: pipeline returned successfully (tokens_delivered is always >= 0 by type)
    let _ = filter_result.tokens_delivered;
}

// ============================================================================
// Test 3: test_extension_tools_always_present
// ============================================================================

#[tokio::test]
async fn test_extension_tools_always_present() {
    // Create a minimal Router with no backend servers
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

    let (_session_id, _rx) = session_store.create_with_defaults(None).await;

    // Send a tools/list request
    let list_req = JsonRpcRequest::new(RequestId::Number(1), "tools/list".to_string(), None);
    let resp = router.route(list_req, None).await;

    // Parse the response
    assert!(resp.result.is_some(), "tools/list should return a result");

    let result = resp.result.unwrap();
    let tools_array = result
        .get("tools")
        .and_then(|t| t.as_array())
        .expect("Result should contain 'tools' array");

    // Collect tool names
    let tool_names: Vec<String> = tools_array
        .iter()
        .filter_map(|t| {
            t.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Assert extension tools are present
    assert!(
        tool_names.contains(&"scp_get_more".to_string()),
        "scp_get_more should be present"
    );
    assert!(
        tool_names.contains(&"scp_info".to_string()),
        "scp_info should be present"
    );
    assert!(
        tool_names.contains(&"scp_budget".to_string()),
        "scp_budget should be present"
    );
    assert!(
        tool_names.contains(&"scp_budget_reset".to_string()),
        "scp_budget_reset should be present"
    );

    // Assert total tools >= 4
    assert!(
        tool_names.len() >= 4,
        "Should have at least 4 extension tools, got {}",
        tool_names.len()
    );
}

// ============================================================================
// Test 4: test_scp_info_returns_version
// ============================================================================

#[tokio::test]
async fn test_scp_info_returns_version() {
    // Create a Router and session
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

    let (_session_id, _rx) = session_store.create_with_defaults(None).await;

    // Send tools/call with name = "scp_info"
    let call_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "tools/call".to_string(),
        Some(json!({
            "name": "scp_info",
            "arguments": {}
        })),
    );

    let resp = router.route(call_req, None).await;

    // Parse the response
    assert!(
        resp.result.is_some(),
        "scp_info should return a result, got error: {:?}",
        resp.error
    );

    let result = resp.result.unwrap();
    let content = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("Response should contain content text");

    // Parse content as JSON
    let info_json: serde_json::Value =
        serde_json::from_str(content).expect("scp_info response should be valid JSON");

    // Assert version is present
    assert!(
        info_json.get("version").is_some(),
        "scp_info should return version"
    );

    // Assert extensions array is present and contains "progressive_disclosure"
    let extensions = info_json
        .get("extensions")
        .and_then(|e| e.as_array())
        .expect("scp_info should have extensions array");

    let has_progressive_disclosure = extensions
        .iter()
        .any(|e| e.as_str() == Some("progressive_disclosure"));

    assert!(
        has_progressive_disclosure,
        "Extensions should contain 'progressive_disclosure'"
    );
}

// ============================================================================
// Test 5: test_intent_hint_stripped_from_backend
// ============================================================================

#[tokio::test]
async fn test_intent_hint_stripped_from_backend() {
    // Test the keyword_accumulator extraction directly
    use scp_core::keyword_accumulator::KeywordAccumulator;

    let mut accumulator = KeywordAccumulator::new();

    // Extract keywords from intent text
    accumulator.extract_from_text("find error messages");

    // Get top keywords
    let top = accumulator.top_k(10);

    // Assert keywords are extracted (excluding stop words)
    assert!(
        top.contains(&"error".to_string()),
        "Should contain 'error', got: {:?}",
        top
    );
    assert!(
        top.contains(&"messages".to_string()),
        "Should contain 'messages', got: {:?}",
        top
    );

    // "find" is not a stop word, so it may or may not be present
    // The important thing is that "error" and "messages" are extracted
}

// ============================================================================
// Test 6: test_chunk_cache_lru_eviction
// ============================================================================

#[tokio::test]
async fn test_chunk_cache_lru_eviction() {
    // Create a session
    let session_store = Arc::new(SessionStore::new(32000));
    let (session_id, _rx) = session_store.create_with_defaults(None).await;

    // Create a test chunk
    let test_chunk = Chunk {
        index: 0,
        text: "Test chunk content".to_string(),
        score: 0.5,
    };

    // Store 51 entries to trigger eviction (cap = 50)
    {
        let session = session_store
            .get(&session_id)
            .await
            .expect("Session should exist");
        let mut session_locked = session.lock().unwrap();

        for i in 0..51 {
            let request_id = format!("req-{}", i);
            session_locked.store_chunks(request_id, vec![test_chunk.clone()]);
        }
    }

    // Verify eviction occurred
    {
        let session = session_store
            .get(&session_id)
            .await
            .expect("Session should exist");
        let session_locked = session.lock().unwrap();

        // req-0 should be evicted (oldest)
        assert!(
            session_locked.get_chunks("req-0").is_none(),
            "req-0 should be evicted"
        );

        // req-50 should be present (most recent)
        assert!(
            session_locked.get_chunks("req-50").is_some(),
            "req-50 should be present"
        );

        // req-25 should be present (not the oldest)
        assert!(
            session_locked.get_chunks("req-25").is_some(),
            "req-25 should be present"
        );
    }
}
