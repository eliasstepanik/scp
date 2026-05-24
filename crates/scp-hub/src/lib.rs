#![warn(missing_docs)]

//! SCP Hub — the central orchestration layer for the SCP system.
//!
//! Manages sessions, routes requests, caches tools, and coordinates filtering.

/// Admin API endpoints.
pub mod admin;
/// SCP extension tools (scp_get_more, scp_info, etc.).
pub mod extension_tools;
/// Main hub orchestration.
pub mod hub;
/// Prometheus metrics.
pub mod metrics;
/// Configuration hot-reload.
pub mod reload;
/// Request routing and fan-out.
pub mod router;
/// Server connection management.
pub mod server_manager;
/// Session storage and management.
pub mod session_store;
/// Tool caching with TTL.
pub mod tool_cache;
/// Tracing and logging setup.
pub mod tracing_setup;
