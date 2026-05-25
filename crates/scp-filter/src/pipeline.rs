use crate::budget::BudgetEnforcer;
use crate::chunker::{Chunk, ChunkSplitter};
use crate::content_type::{ContentType, ContentTypeRouter};
use crate::dedup::DedupFilter;
use crate::dedup::DeliveryLog;
use crate::delivery_logger::DeliveryLogger;
use crate::embedding_scorer::EmbeddingChunkScorer;
use crate::progressive::ProgressiveDisclosureAnnotator;
use crate::relevance::RelevanceScorer;
use crate::token_count::count_tokens;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Context passed to the pipeline for each tool call
pub struct FilterContext {
    /// Session identifier
    pub session_id: String,
    /// Name of the tool being called
    pub tool_name: String,
    /// Token budget for this response
    pub budget_tokens: usize,
    /// Query terms for relevance scoring
    pub query_terms: Vec<String>,
    /// Delivery log for deduplication
    pub delivery_log: Arc<Mutex<DeliveryLog>>,
    /// Short-circuit threshold in tokens
    pub short_circuit_below_tokens: usize,
    /// Request identifier
    pub request_id: String,
}

/// Result returned by the pipeline
pub struct FilterResult {
    /// Filtered content to deliver
    pub content: String,
    /// Total tokens received from the tool
    pub tokens_received: usize,
    /// Tokens delivered to the user
    pub tokens_delivered: usize,
    /// Tokens saved by filtering
    pub tokens_saved: usize,
    /// Number of chunks shown
    pub chunks_shown: usize,
    /// Total number of chunks
    pub chunks_total: usize,
    /// Chunks that were dropped
    pub dropped_chunks: Vec<Chunk>,
}

/// Main filtering pipeline for processing tool responses
pub struct FilterPipeline {
    /// Whether filtering is enabled
    pub enabled: bool,
    /// Short-circuit threshold in tokens
    pub short_circuit_below_tokens: usize,
    /// Whether progressive disclosure is enabled
    pub progressive_disclosure_enabled: bool,
    /// Hint text for progressive disclosure
    pub progressive_hint_text: String,
    /// Relevance scoring engine: "tfidf" or "embedding"
    pub relevance_engine: String,
    /// Optional embedding-based chunk scorer
    pub embedding_scorer: Option<Arc<EmbeddingChunkScorer>>,
}

impl FilterPipeline {
    /// Create a new filter pipeline from configuration.
    pub fn new(config: &scp_core::config::FilterConfig) -> Self {
        let embedding_scorer = if config.relevance_engine == "embedding" {
            Some(Arc::new(EmbeddingChunkScorer::new(&config.embedding)))
        } else {
            None
        };

        Self {
            enabled: config.enabled,
            short_circuit_below_tokens: config.short_circuit_below_tokens,
            progressive_disclosure_enabled: config.progressive_disclosure_enabled,
            progressive_hint_text: config.progressive_hint_text.clone(),
            relevance_engine: config.relevance_engine.clone(),
            embedding_scorer,
        }
    }

    /// Run the full 8-stage pipeline on a tool response content value.
    pub async fn run(&self, content: &Value, ctx: &FilterContext) -> FilterResult {
        // Stage 1: Extract text and classify content type
        let text_str = extract_text(content);
        let content_type = ContentTypeRouter::classify(content);

        // If Image or Binary, pass through unchanged
        if matches!(content_type, ContentType::Image | ContentType::Binary) {
            let tokens_received = count_tokens(&text_str);
            return FilterResult {
                content: text_str,
                tokens_received,
                tokens_delivered: tokens_received,
                tokens_saved: 0,
                chunks_shown: 1,
                chunks_total: 1,
                dropped_chunks: vec![],
            };
        }

        // Stage 2: Token measurement + short-circuit
        let tokens_received = count_tokens(&text_str);
        if !self.enabled || tokens_received <= self.short_circuit_below_tokens {
            return FilterResult {
                content: text_str,
                tokens_received,
                tokens_delivered: tokens_received,
                tokens_saved: 0,
                chunks_shown: 1,
                chunks_total: 1,
                dropped_chunks: vec![],
            };
        }

        // Stage 3: Dedup check on full content
        let full_hash = DedupFilter::hash_text(&text_str);
        // Extract data from ctx before any async operations to avoid holding references
        let delivery_log = ctx.delivery_log.clone();
        let query_terms = ctx.query_terms.clone();
        let request_id = ctx.request_id.clone();

        let is_duplicate = {
            let delivery_log_guard = delivery_log.lock().unwrap_or_else(|e| e.into_inner());
            delivery_log_guard.contains(&full_hash)
        };

        if is_duplicate {
            return FilterResult {
                content: String::new(),
                tokens_received,
                tokens_delivered: 0,
                tokens_saved: tokens_received,
                chunks_shown: 0,
                chunks_total: 1,
                dropped_chunks: vec![],
            };
        }

        // Stage 4: ChunkSplitter
        let splitter = ChunkSplitter::auto(&text_str);
        let mut chunks = splitter.split(&text_str);

        // If no chunks, return empty
        if chunks.is_empty() {
            return FilterResult {
                content: String::new(),
                tokens_received,
                tokens_delivered: 0,
                tokens_saved: tokens_received,
                chunks_shown: 0,
                chunks_total: 0,
                dropped_chunks: vec![],
            };
        }

        // Stage 5: RelevanceScorer or EmbeddingChunkScorer
        if self.relevance_engine == "embedding" && self.embedding_scorer.is_some() {
            let query_string = query_terms.join(" ");
            if let Some(scorer) = &self.embedding_scorer {
                scorer
                    .score_chunks(&mut chunks, &query_string, &query_terms)
                    .await;
            }
        } else {
            RelevanceScorer::score_chunks(&mut chunks, &query_terms);
        }

        // Stage 6: BudgetEnforcer — one-pass: selected, dropped, total returned directly
        let (selected_chunks, dropped_chunks, total_count) =
            BudgetEnforcer::select_chunks(chunks, ctx.budget_tokens, 200);

        // If no chunks selected, return empty
        if selected_chunks.is_empty() {
            return FilterResult {
                content: String::new(),
                tokens_received,
                tokens_delivered: 0,
                tokens_saved: tokens_received,
                chunks_shown: 0,
                chunks_total: total_count,
                dropped_chunks: vec![],
            };
        }

        // Stage 7: ProgressiveDisclosureAnnotator
        let assembled = BudgetEnforcer::reassemble(&selected_chunks);
        let annotator = ProgressiveDisclosureAnnotator::new(self.progressive_disclosure_enabled);
        let (final_content, returned_dropped_chunks) = annotator.annotate(
            assembled,
            selected_chunks.len(),
            dropped_chunks,
            &request_id,
            &self.progressive_hint_text,
        );

        // Stage 8: DeliveryLogger
        let mut delivery_log_guard = delivery_log.lock().unwrap_or_else(|e| e.into_inner());
        // Record the full content hash to prevent duplicate full responses
        delivery_log_guard.insert(full_hash);
        // Also record individual chunk hashes
        DeliveryLogger::record(&selected_chunks, &mut delivery_log_guard);
        drop(delivery_log_guard);

        let tokens_delivered = count_tokens(&final_content);

        FilterResult {
            content: final_content,
            tokens_received,
            tokens_delivered,
            tokens_saved: tokens_received.saturating_sub(tokens_delivered),
            chunks_shown: selected_chunks.len(),
            chunks_total: total_count,
            dropped_chunks: returned_dropped_chunks,
        }
    }
}

/// Extract text from a serde_json::Value
fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            // MCP format: [{type: "text", text: "..."}]
            arr.iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => content.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context(session_id: &str) -> FilterContext {
        FilterContext {
            session_id: session_id.to_string(),
            tool_name: "test_tool".to_string(),
            budget_tokens: 1000,
            query_terms: vec!["test".to_string(), "content".to_string()],
            delivery_log: Arc::new(Mutex::new(DeliveryLog::new(100))),
            short_circuit_below_tokens: 500,
            request_id: "test-req-1".to_string(),
        }
    }

    fn create_test_config(enabled: bool) -> scp_core::config::FilterConfig {
        scp_core::config::FilterConfig {
            enabled,
            budget_strategy: "truncate".to_string(),
            chunking_strategy: "paragraph".to_string(),
            relevance_engine: "tags".to_string(),
            progressive_disclosure_enabled: true,
            short_circuit_below_tokens: 500,
            embedding: Default::default(),
            progressive_hint_text: "[SCP: {shown} of {total} results shown. Call scp_get_more(request_id=\"{id}\") for more.]".to_string(),
        }
    }

    #[tokio::test]
    async fn test_pipeline_short_circuit_small_content() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");
        let content = Value::String("short".to_string());

        let result = pipeline.run(&content, &ctx).await;

        assert_eq!(result.content, "short");
        assert_eq!(result.tokens_saved, 0);
        assert!(result.tokens_received <= 500);
    }

    #[tokio::test]
    async fn test_pipeline_image_passthrough() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");
        let image_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let content = Value::String(image_uri.to_string());

        let result = pipeline.run(&content, &ctx).await;

        assert_eq!(result.content, image_uri);
        assert_eq!(result.tokens_saved, 0);
    }

    #[tokio::test]
    async fn test_pipeline_disabled_passthrough() {
        let pipeline = FilterPipeline::new(&create_test_config(false));
        let ctx = create_test_context("session1");
        let large_text = "word ".repeat(200); // Large content
        let content = Value::String(large_text.clone());

        let result = pipeline.run(&content, &ctx).await;

        assert_eq!(result.content, large_text);
        assert_eq!(result.tokens_saved, 0);
    }

    #[tokio::test]
    async fn test_pipeline_full_run() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");

        // Create large content with query terms
        let large_text = "This is a test content about testing.\n\nMore test content here.\n\nUnrelated paragraph about something else.".repeat(10);
        let content = Value::String(large_text);

        let result = pipeline.run(&content, &ctx).await;

        // Should have filtered content
        assert!(!result.content.is_empty());
        assert!(result.tokens_delivered <= result.tokens_received);
        // With relevance filtering, we should save some tokens (or at least not increase)
        assert!(result.tokens_saved <= result.tokens_received);
        assert!(result.chunks_shown > 0);
        assert!(result.chunks_total > 0);
    }

    #[tokio::test]
    async fn test_pipeline_dedup_second_call() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let shared_log = Arc::new(Mutex::new(DeliveryLog::new(100)));

        let ctx1 = FilterContext {
            session_id: "session1".to_string(),
            tool_name: "test_tool".to_string(),
            budget_tokens: 1000,
            query_terms: vec!["test".to_string()],
            delivery_log: Arc::clone(&shared_log),
            short_circuit_below_tokens: 500,
            request_id: "test-req-1".to_string(),
        };

        // Create large content to avoid short-circuit
        let content =
            Value::String("This is test content that should be deduplicated. ".repeat(50));

        // First call
        let result1 = pipeline.run(&content, &ctx1).await;
        assert!(!result1.content.is_empty());

        // Second call with same content - should be deduplicated
        let result2 = pipeline.run(&content, &ctx1).await;
        // After first delivery, the full content hash is in the log
        // So second call should return empty
        assert_eq!(result2.content, "");
        assert_eq!(result2.tokens_delivered, 0);
        assert_eq!(result2.tokens_saved, result2.tokens_received);
    }

    #[tokio::test]
    async fn test_pipeline_mcp_array_format() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");

        let content = serde_json::json!([
            {"type": "text", "text": "First paragraph with test content."},
            {"type": "text", "text": "Second paragraph with more test information."}
        ]);

        let result = pipeline.run(&content, &ctx).await;

        assert!(!result.content.is_empty());
        assert!(result.content.contains("First paragraph"));
        assert!(result.content.contains("Second paragraph"));
    }

    #[tokio::test]
    async fn test_pipeline_empty_content() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");
        let content = Value::String(String::new());

        let result = pipeline.run(&content, &ctx).await;

        assert_eq!(result.content, "");
        assert_eq!(result.tokens_received, 0);
    }

    #[tokio::test]
    async fn test_pipeline_json_object_passthrough() {
        let pipeline = FilterPipeline::new(&create_test_config(true));
        let ctx = create_test_context("session1");

        let content = serde_json::json!({
            "key": "value",
            "nested": {
                "data": "test"
            }
        });

        let result = pipeline.run(&content, &ctx).await;

        // JSON objects are classified as StructuredJson and should be processed
        assert!(!result.content.is_empty());
    }

    #[test]
    fn test_extract_text_string() {
        let content = Value::String("hello world".to_string());
        let text = extract_text(&content);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_extract_text_mcp_array() {
        let content = serde_json::json!([
            {"type": "text", "text": "first"},
            {"type": "text", "text": "second"}
        ]);
        let text = extract_text(&content);
        assert_eq!(text, "first\nsecond");
    }

    #[test]
    fn test_extract_text_json_object() {
        let content = serde_json::json!({"key": "value"});
        let text = extract_text(&content);
        assert!(text.contains("key"));
        assert!(text.contains("value"));
    }
}
