#![warn(missing_docs)]

//! Tool indexing and discovery for the SCP system.
//!
//! Provides tool registry, semantic search, and scoring mechanisms for ranking tools
//! based on relevance, usage patterns, and embeddings.

/// Embedding cache for performance.
pub mod embedding_cache;
/// Embedding client for semantic search.
pub mod embedding_client;
/// Embedding-based tool scoring.
pub mod embedding_scorer;
/// Tool registry and metadata management.
pub mod registry;
/// Tool scoring and ranking pipeline.
pub mod scorer;
/// Tag-based tool scoring.
pub mod tags;
/// TF-IDF based tool indexing.
pub mod tfidf;
/// Tool usage tracking and statistics.
pub mod usage;

pub use embedding_cache::EmbeddingCache;
pub use embedding_client::{EmbeddingClient, EmbeddingError};
pub use embedding_scorer::EmbeddingToolScorer;
pub use registry::{RegistryError, ToolEntry, ToolRegistry};
pub use scorer::{ScoredTool, ScoringPipeline};
pub use tags::TagScorer;
pub use tfidf::TfIdfIndex;
pub use usage::UsageTracker;
