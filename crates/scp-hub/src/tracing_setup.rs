use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
use scp_core::config::LoggingConfig;

/// Output format for tracing logs.
#[derive(Debug, Clone, Copy)]
pub enum TracingFormat {
    /// JSON-formatted logs.
    Json,
    /// Pretty-printed logs with colors and formatting.
    Pretty,
}

/// Initialize the tracing subscriber with the specified format and log level.
///
/// # Arguments
///
/// * `format` - The output format for logs (JSON or Pretty)
/// * `level` - The log level filter (e.g., "debug", "info", "warn", "error")
pub fn init_tracing(format: TracingFormat, level: &str) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    match format {
        TracingFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_span_events(FmtSpan::CLOSE),
                )
                .init();
        }
        TracingFormat::Pretty => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .pretty()
                        .with_span_events(FmtSpan::CLOSE),
                )
                .init();
        }
    }
}

/// Initialize tracing from LoggingConfig
pub fn init_tracing_from_config(config: &LoggingConfig) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    if config.json_format {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_span_events(FmtSpan::CLOSE),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_span_events(FmtSpan::CLOSE),
            )
            .init();
    }
}
