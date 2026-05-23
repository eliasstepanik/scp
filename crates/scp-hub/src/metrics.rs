use once_cell::sync::Lazy;
use prometheus::{
    Counter, CounterVec, Gauge, Histogram, HistogramOpts, IntGauge, Opts, Registry, TextEncoder,
};

/// Global Prometheus metrics registry.
pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

/// Total tokens saved by filtering
#[allow(dead_code)]
pub static SCP_TOKENS_SAVED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    let counter = Counter::with_opts(Opts::new(
        "scp_tokens_saved_total",
        "Total tokens saved by filtering",
    ))
    .expect("failed to create scp_tokens_saved_total counter — this is a bug in SCP startup");
    REGISTRY.register(Box::new(counter.clone())).expect("failed to register scp_tokens_saved_total — this is a bug in SCP startup");
    counter
});

/// Total tokens delivered to clients
#[allow(dead_code)]
pub static SCP_TOKENS_DELIVERED_TOTAL: Lazy<Counter> = Lazy::new(|| {
    let counter = Counter::with_opts(Opts::new(
        "scp_tokens_delivered_total",
        "Total tokens delivered to clients",
    ))
    .expect("failed to create scp_tokens_delivered_total counter — this is a bug in SCP startup");
    REGISTRY.register(Box::new(counter.clone())).expect("failed to register scp_tokens_delivered_total — this is a bug in SCP startup");
    counter
});

/// Times embedding scorer fell back to TF-IDF
#[allow(dead_code)]
pub static SCP_EMBEDDING_FALLBACK_TOTAL: Lazy<Counter> = Lazy::new(|| {
    let counter = Counter::with_opts(Opts::new(
        "scp_embedding_fallback_total",
        "Times embedding scorer fell back to TF-IDF",
    ))
    .expect("failed to create scp_embedding_fallback_total counter — this is a bug in SCP startup");
    REGISTRY.register(Box::new(counter.clone())).expect("failed to register scp_embedding_fallback_total — this is a bug in SCP startup");
    counter
});

/// Request duration in seconds
#[allow(dead_code)]
pub static SCP_REQUEST_DURATION_SECONDS: Lazy<Histogram> = Lazy::new(|| {
    let opts = HistogramOpts::new("scp_request_duration_seconds", "Request duration in seconds")
        .buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0]);
    let hist = Histogram::with_opts(opts).expect("failed to create scp_request_duration_seconds histogram — this is a bug in SCP startup");
    REGISTRY.register(Box::new(hist.clone())).expect("failed to register scp_request_duration_seconds — this is a bug in SCP startup");
    hist
});

/// Total errors by kind
#[allow(dead_code)]
pub static SCP_ERRORS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    let counter = CounterVec::new(Opts::new("scp_errors_total", "Total errors by kind"), &["kind"])
        .expect("failed to create scp_errors_total counter — this is a bug in SCP startup");
    REGISTRY.register(Box::new(counter.clone())).expect("failed to register scp_errors_total — this is a bug in SCP startup");
    counter
});

/// Active pool connections
#[allow(dead_code)]
pub static SCP_POOL_CONNECTIONS_ACTIVE: Lazy<Gauge> = Lazy::new(|| {
    let gauge = Gauge::with_opts(Opts::new(
        "scp_pool_connections_active",
        "Active pool connections",
    ))
    .expect("failed to create scp_pool_connections_active gauge — this is a bug in SCP startup");
    REGISTRY.register(Box::new(gauge.clone())).expect("failed to register scp_pool_connections_active — this is a bug in SCP startup");
    gauge
});

/// Number of in-flight MCP requests
#[allow(dead_code)]
pub static SCP_INFLIGHT_REQUESTS: Lazy<IntGauge> = Lazy::new(|| {
    let gauge = IntGauge::new("scp_inflight_requests", "Number of in-flight MCP requests").expect("failed to create scp_inflight_requests gauge — this is a bug in SCP startup");
    REGISTRY.register(Box::new(gauge.clone())).expect("failed to register scp_inflight_requests — this is a bug in SCP startup");
    gauge
});

/// Serialize all metrics to Prometheus text format
pub fn gather_metrics() -> String {
    // Force initialization of all metrics by accessing them
    let _ = &*SCP_TOKENS_SAVED_TOTAL;
    let _ = &*SCP_TOKENS_DELIVERED_TOTAL;
    let _ = &*SCP_EMBEDDING_FALLBACK_TOTAL;
    let _ = &*SCP_REQUEST_DURATION_SECONDS;
    let _ = &*SCP_ERRORS_TOTAL;
    let _ = &*SCP_POOL_CONNECTIONS_ACTIVE;
    let _ = &*SCP_INFLIGHT_REQUESTS;

    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = String::new();
    encoder
        .encode_utf8(&metric_families, &mut buffer)
        .unwrap_or_default();
    buffer
}
