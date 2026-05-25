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
// Test 1: test_scp_get_more_returns_stored_chunks
// ============================================================================
// Verify that chunks stored in a session are retrievable via the scp_get_more
// extension tool route, and that an unknown request_id returns empty results.
#[tokio::test]
async fn test_scp_get_more_returns_stored_chunks() {
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
    let session_store = Arc::new(SessionStore::new(32000));

    let router = Arc::new(Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        300,  // fanout_timeout_secs
        4000, // request_token_budget
        make_filter_pipeline(),
    ));

    // Create a session and store a chunk in it
    let (session_id, _rx) = session_store.create_with_defaults(None).await;
    let session_arc = session_store
        .get(&session_id)
        .await
        .expect("Session should exist");

    {
        let mut s = session_arc.lock().unwrap();
        s.store_chunks(
            "req-abc".to_string(),
            vec![Chunk {
                text: "chunk text".to_string(),
                score: 0.9,
                index: 0,
            }],
        );
    }

    // Route a scp_get_more request with the stored request_id
    let get_more_req = JsonRpcRequest::new(
        RequestId::Number(1),
        "tools/call".to_string(),
        Some(json!({
            "name": "scp_get_more",
            "arguments": {
                "request_id": "req-abc",
                "offset": 0,
                "limit": 10
            }
        })),
    );

    let resp = router.route(get_more_req, Some(session_arc.clone())).await;

    assert!(
        resp.result.is_some(),
        "scp_get_more should return a result, got: {:?}",
        resp.error
    );

    // The result is wrapped in MCP content: {"content": [{"type": "text", "text": "<json>"}]}
    let result = resp.result.unwrap();
    let text = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("Response should contain content[0].text");

    let parsed: serde_json::Value = serde_json::from_str(text).expect("text should be valid JSON");

    let items = parsed
        .get("items")
        .and_then(|v| v.as_array())
        .expect("Response should contain items array");
    assert_eq!(items.len(), 1, "Should have 1 item");
    assert_eq!(
        items[0].as_str().unwrap_or(""),
        "chunk text",
        "Item text should match stored chunk"
    );

    let total = parsed
        .get("total")
        .and_then(|v| v.as_u64())
        .expect("Response should contain total");
    assert_eq!(total, 1, "total should be 1");

    let has_more = parsed
        .get("has_more")
        .and_then(|v| v.as_bool())
        .expect("Response should contain has_more");
    assert!(
        !has_more,
        "has_more should be false for 1 item with limit 10"
    );

    // -----------------------------------------------------------------------
    // Now test unknown request_id → total: 0, empty items, no error
    // -----------------------------------------------------------------------
    let unknown_req = JsonRpcRequest::new(
        RequestId::Number(2),
        "tools/call".to_string(),
        Some(json!({
            "name": "scp_get_more",
            "arguments": {
                "request_id": "nonexistent-req-xyz",
                "offset": 0,
                "limit": 10
            }
        })),
    );

    let unknown_resp = router.route(unknown_req, Some(session_arc.clone())).await;

    assert!(
        unknown_resp.result.is_some(),
        "Unknown request_id should still return a result (not an error)"
    );
    assert!(
        unknown_resp.error.is_none(),
        "Unknown request_id should not return an error"
    );

    let unknown_result = unknown_resp.result.unwrap();
    let unknown_text = unknown_result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("Unknown response should contain content[0].text");

    let unknown_parsed: serde_json::Value =
        serde_json::from_str(unknown_text).expect("unknown text should be valid JSON");

    let unknown_total = unknown_parsed
        .get("total")
        .and_then(|v| v.as_u64())
        .expect("Response should contain total");
    assert_eq!(unknown_total, 0, "total should be 0 for unknown request_id");

    let unknown_items = unknown_parsed
        .get("items")
        .and_then(|v| v.as_array())
        .expect("Response should contain items array");
    assert!(
        unknown_items.is_empty(),
        "items should be empty for unknown request_id"
    );
}

// ============================================================================
// Test 2: test_pipeline_dropped_chunks_land_in_session
// ============================================================================
// Verify that when a large content is processed through a budget-constrained
// pipeline, dropped chunks are non-empty and can be stored/retrieved via Session.
#[tokio::test]
async fn test_pipeline_dropped_chunks_land_in_session() {
    // Build large content: ~10 paragraphs of ~600 chars each
    let paragraph = "word ".repeat(120); // ~120 words ≈ 600 chars per paragraph
    let big_content: String = (0..10)
        .map(|_| paragraph.clone())
        .collect::<Vec<_>>()
        .join("\n\n");

    // Create a FilterConfig with a tiny short_circuit threshold so the pipeline
    // actually runs (content must exceed short_circuit_below_tokens).
    let filter_config = FilterConfig {
        enabled: true,
        budget_strategy: "truncate".to_string(),
        chunking_strategy: "paragraph".to_string(),
        relevance_engine: "tags".to_string(),
        // Set threshold very low so the pipeline doesn't short-circuit
        short_circuit_below_tokens: 10,
        progressive_disclosure_enabled: true,
        progressive_hint_text:
            "[SCP: {shown} of {total} results shown. Call scp_get_more(request_id=\"{id}\") for more.]"
                .to_string(),
        embedding: Default::default(),
    };

    let pipeline = FilterPipeline::new(&filter_config);

    // Build a FilterContext with a tiny budget (50 tokens) to force chunk dropping
    let ctx = FilterContext {
        session_id: "test-session-pipeline".to_string(),
        tool_name: "test_tool".to_string(),
        // 50 tokens is far below the content size, forcing most chunks to be dropped
        budget_tokens: 50,
        query_terms: vec!["word".to_string()],
        delivery_log: Arc::new(Mutex::new(DeliveryLog::new(1000))),
        short_circuit_below_tokens: 10,
        request_id: "test-req-pipeline-1".to_string(),
    };

    let content_value = serde_json::Value::String(big_content.clone());
    let filter_result = pipeline.run(&content_value, &ctx).await;

    assert!(
        !filter_result.dropped_chunks.is_empty(),
        "Expected dropped chunks with 50-token budget on ~1200-token content, got 0 dropped; \
         chunks_total={}, chunks_shown={}",
        filter_result.chunks_total,
        filter_result.chunks_shown
    );

    // Store dropped chunks in a session and verify retrieval
    let session_store = Arc::new(SessionStore::new(32000));
    let (session_id, _rx) = session_store.create_with_defaults(None).await;
    let session_arc = session_store
        .get(&session_id)
        .await
        .expect("Session should exist");

    let dropped_count = filter_result.dropped_chunks.len();
    {
        let mut s = session_arc.lock().unwrap();
        s.store_chunks(
            "test-req-pipeline-1".to_string(),
            filter_result.dropped_chunks,
        );
    }

    // Retrieve and assert
    {
        let s = session_arc.lock().unwrap();
        let retrieved = s
            .get_chunks("test-req-pipeline-1")
            .expect("Chunks should be retrievable after store");

        assert_eq!(
            retrieved.len(),
            dropped_count,
            "Retrieved chunk count should match stored count"
        );

        // Every retrieved chunk should have non-empty text
        for chunk in retrieved {
            assert!(!chunk.text.is_empty(), "Chunk text should not be empty");
        }
    }

    // Verify a different request_id returns None
    {
        let s = session_arc.lock().unwrap();
        assert!(
            s.get_chunks("nonexistent-req").is_none(),
            "get_chunks for unknown request_id should return None"
        );
    }
}
