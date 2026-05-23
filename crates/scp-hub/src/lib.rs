#![warn(missing_docs)]

//! SCP Hub — the central orchestration layer for the SCP system.
//!
//! Manages sessions, routes requests, caches tools, and coordinates filtering.

/// Session storage and management.
pub mod session_store;
/// Request routing and fan-out.
pub mod router;
/// Server connection management.
pub mod server_manager;
/// Admin API endpoints.
pub mod admin;
/// Configuration hot-reload.
pub mod reload;
/// Main hub orchestration.
pub mod hub;
/// Tracing and logging setup.
pub mod tracing_setup;
/// Tool caching with TTL.
pub mod tool_cache;
/// SCP extension tools (scp_get_more, scp_info, etc.).
pub mod extension_tools;
/// Prometheus metrics.
pub mod metrics;
