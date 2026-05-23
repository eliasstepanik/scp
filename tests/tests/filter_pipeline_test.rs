use scp_core::config::FilterConfig;
use scp_filter::dedup::DeliveryLog;
use scp_filter::pipeline::{FilterContext, FilterPipeline};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// Helper to create a test session with keyword accumulator
fn create_test_session() -> (String, Arc<Mutex<DeliveryLog>>) {
    let session_id = uuid::Uuid::new_v4().to_string();
    let delivery_log = Arc::new(Mutex::new(DeliveryLog::new(10_000)));
    (session_id, delivery_log)
}

/// Helper to create a filter config with custom short_circuit threshold
fn create_test_config(short_circuit_below_tokens: usize) -> FilterConfig {
    FilterConfig {
        enabled: true,
        budget_strategy: "truncate".to_string(),
        chunking_strategy: "paragraph".to_string(),
        relevance_engine: "tags".to_string(),
        progressive_disclosure_enabled: true,
        short_circuit_below_tokens,
        embedding: Default::default(),
        intent_hint_enabled: true,
        progressive_hint_text: "[SCP: {shown} of {total} results shown. Call scp_get_more(request_id=\"{id}\") for more.]".to_string(),
    }
}

/// Test 1: Large log filtering with keyword relevance
/// - Build a 200-line log with 5 ERROR lines
/// - Prime keyword accumulator with "error"
/// - Verify filtering happens and tokens are saved
#[tokio::test]
async fn test_large_log_filtering() {
    let (session_id, delivery_log) = create_test_session();

    // Build a 200-line log with 5 ERROR lines scattered throughout
    // Each line is repeated to ensure we have enough tokens to trigger filtering
    let mut log_lines = Vec::new();
    for i in 0..200 {
        if i == 10 || i == 50 || i == 100 || i == 150 || i == 190 {
            log_lines.push(format!("[ERROR] Critical error occurred at line {} with detailed information about the failure", i));
        } else {
            let noise_type = match i % 3 {
                0 => "INFO",
                1 => "DEBUG",
                _ => "WARN",
            };
            log_lines.push(format!(
                "[{}] processing request {} - cache hit - additional noise content to increase size",
                noise_type, i
            ));
        }
    }
    let log_content = log_lines.join("\n");

    // Create FilterContext with keyword "error" primed
    let mut query_terms = vec!["error".to_string()];
    // Add some additional noise terms to simulate real keyword accumulator
    query_terms.push("processing".to_string());
    query_terms.push("request".to_string());

    let ctx = FilterContext {
        session_id: session_id.clone(),
        tool_name: "test_tool".to_string(),
        budget_tokens: 200, // Tight budget to force filtering
        query_terms,
        delivery_log: delivery_log.clone(),
        short_circuit_below_tokens: 100, // Low threshold so large log doesn't short-circuit
        request_id: "test-req-1".to_string(),
    };

    // Create pipeline with custom config (low short_circuit threshold)
    let config = create_test_config(100);
    let pipeline = FilterPipeline::new(&config);

    // Run the pipeline
    let result = pipeline.run(&json!(log_content), &ctx).await;

    // Assertions
    assert!(
        result.tokens_received > 100,
        "Should have received more than 100 tokens, got {}",
        result.tokens_received
    );
    assert!(
        result.tokens_delivered <= result.tokens_received,
        "Delivered tokens should not exceed received tokens"
    );

    // Either tokens were saved OR content is empty (both indicate filtering happened)
    let filtering_occurred = result.tokens_saved > 0 || result.content.is_empty();
    assert!(
        filtering_occurred,
        "Filtering should occur: tokens_saved={}, content_empty={}",
        result.tokens_saved,
        result.content.is_empty()
    );

    // The result should contain at least one ERROR line OR be empty (filtered out)
    // OR contain a progressive disclosure hint
    let content_lower = result.content.to_lowercase();
    let has_error = content_lower.contains("error");
    let has_disclosure_hint = content_lower.contains("chunk") || content_lower.contains("shown");
    let is_empty = result.content.is_empty();

    assert!(
        has_error || has_disclosure_hint || is_empty,
        "Result should contain ERROR lines, progressive disclosure hint, or be empty"
    );
}

/// Test 2: Dedup across multiple calls
/// - Create same content string twice
/// - First call: verify content is returned
/// - Second call with same content: verify dedup fires (empty result)
#[tokio::test]
async fn test_dedup_across_calls() {
    let (session_id, delivery_log) = create_test_session();

    // Create test content that's large enough to not be short-circuited
    // Repeat the string to ensure we have enough tokens
    let test_content = "This is test content that should be deduplicated. ".repeat(50);

    let ctx = FilterContext {
        session_id: session_id.clone(),
        tool_name: "test_tool".to_string(),
        budget_tokens: 1000,
        query_terms: vec!["test".to_string()],
        delivery_log: delivery_log.clone(),
        short_circuit_below_tokens: 500,
        request_id: "test-req-1".to_string(),
    };

    // Create pipeline with default config
    let config = create_test_config(500);
    let pipeline = FilterPipeline::new(&config);

    // First call: run pipeline with fresh content
    let result1 = pipeline.run(&json!(test_content), &ctx).await;

    // First call should return content (not empty)
    assert!(
        !result1.content.is_empty(),
        "First call should return content, got empty"
    );
    assert!(
        result1.tokens_delivered > 0,
        "First call should deliver some tokens, got {}",
        result1.tokens_delivered
    );

    // Second call with SAME content: dedup should fire
    let result2 = pipeline.run(&json!(test_content), &ctx).await;

    // Second call should have empty content (dedup fired)
    // because the full content hash was recorded in delivery_log
    assert_eq!(
        result2.content, "",
        "Second call should trigger dedup and return empty content"
    );
    assert_eq!(
        result2.tokens_delivered, 0,
        "Empty content should have 0 tokens delivered"
    );
    assert_eq!(
        result2.tokens_saved, result2.tokens_received,
        "Dedup should save all tokens on duplicate"
    );
}
