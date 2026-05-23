use criterion::{black_box, criterion_group, criterion_main, Criterion};
use scp_filter::{
    FilterPipeline, FilterContext, token_count::count_tokens, chunker::Chunk,
    relevance::RelevanceScorer, dedup::DeliveryLog,
};
use scp_core::config::FilterConfig;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// Create a test FilterContext with standard settings
fn create_test_context(session_id: &str) -> FilterContext {
    FilterContext {
        session_id: session_id.to_string(),
        tool_name: "benchmark_tool".to_string(),
        budget_tokens: 5000,
        query_terms: vec![
            "test".to_string(),
            "content".to_string(),
            "data".to_string(),
        ],
        delivery_log: Arc::new(Mutex::new(DeliveryLog::new(1000))),
        short_circuit_below_tokens: 500,
        request_id: "bench-req-1".to_string(),
    }
}

/// Create a test FilterConfig with default settings
fn create_test_config() -> FilterConfig {
    FilterConfig {
        enabled: true,
        budget_strategy: "truncate".to_string(),
        chunking_strategy: "paragraph".to_string(),
        relevance_engine: "tags".to_string(),
        progressive_disclosure_enabled: true,
        short_circuit_below_tokens: 500,
        embedding: Default::default(),
        intent_hint_enabled: true,
        progressive_hint_text: "[SCP: {shown} of {total} results shown]".to_string(),
    }
}

/// Generate a JSON array of tool objects (simulating tool list response)
fn generate_tool_array(count: usize) -> Value {
    let tools: Vec<Value> = (0..count)
        .map(|i| {
            json!({
                "type": "tool",
                "name": format!("tool_{}", i),
                "description": format!(
                    "This is a test tool number {}. It performs various operations on data. \
                     It can process inputs and generate outputs. This tool is useful for testing \
                     the filtering pipeline with realistic tool descriptions. ",
                    i
                ),
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "param1": {"type": "string"},
                        "param2": {"type": "number"},
                        "param3": {"type": "boolean"}
                    }
                }
            })
        })
        .collect();
    Value::Array(tools)
}

/// Benchmark: Filter pipeline on small response (10 tools)
fn bench_filter_pipeline_small(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("filter_pipeline_small_10_tools", |b| {
        b.to_async(&rt).iter(|| async {
            let pipeline = FilterPipeline::new(&create_test_config());
            let ctx = create_test_context("session_small");
            let content = black_box(generate_tool_array(10));

            pipeline.run(&content, &ctx).await
        });
    });
}

/// Benchmark: Filter pipeline on large response (100 tools)
fn bench_filter_pipeline_large(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("filter_pipeline_large_100_tools", |b| {
        b.to_async(&rt).iter(|| async {
            let pipeline = FilterPipeline::new(&create_test_config());
            let ctx = create_test_context("session_large");
            let content = black_box(generate_tool_array(100));

            pipeline.run(&content, &ctx).await
        });
    });
}

/// Benchmark: Token counting on 10KB text payload
fn bench_token_counting(c: &mut Criterion) {
    // Generate a 10KB text payload
    let text = "This is a sample text for token counting benchmarks. \
                It contains multiple paragraphs and sentences to simulate realistic content. \
                Token counting is an important operation in the filtering pipeline. \
                It helps determine if content should be short-circuited or processed further. \
                The heuristic-based token counter uses byte ratios to estimate token counts. \
                This is much faster than using a real tokenizer like tiktoken or sentencepiece. \
                "
        .repeat(200); // Repeat to reach ~10KB

    c.bench_function("token_counting_10kb", |b| {
        b.iter(|| {
            let text_ref = black_box(&text);
            count_tokens(text_ref)
        });
    });
}

/// Benchmark: Relevance scoring on 50 chunks
fn bench_embedding_scorer(c: &mut Criterion) {
    c.bench_function("relevance_scorer_50_chunks", |b| {
        b.iter(|| {
            let mut chunks = (0..50)
                .map(|i| {
                    Chunk::new(
                        format!(
                            "This is chunk number {}. It contains test content about data processing. \
                             The chunk includes information about testing and content filtering. \
                             This is useful for benchmarking the relevance scoring engine. ",
                            i
                        ),
                        i,
                    )
                })
                .collect::<Vec<_>>();

            let query_terms = black_box(vec![
                "test".to_string(),
                "content".to_string(),
                "data".to_string(),
            ]);

            RelevanceScorer::score_chunks(&mut chunks, &query_terms);
            chunks
        });
    });
}

criterion_group!(
    benches,
    bench_filter_pipeline_small,
    bench_filter_pipeline_large,
    bench_token_counting,
    bench_embedding_scorer
);
criterion_main!(benches);
