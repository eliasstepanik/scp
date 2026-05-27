#![warn(missing_docs)]

//! Content filtering and chunking for the SCP system.
//!
//! Provides tools for filtering, chunking, and scoring content based on relevance,
//! embeddings, and delivery constraints.

/// Budget enforcement — truncates responses to fit within token budgets.
pub mod budget;
/// Content chunking strategies and chunk splitting.
pub mod chunker;
/// Content type classification and routing.
pub mod content_type;
/// Deduplication filtering and delivery log tracking.
pub mod dedup;
/// Delivery logging for tracking sent content.
pub mod delivery_logger;
/// Embedding-based chunk scoring using cosine similarity.
pub mod embedding_scorer;
/// JSON field stripping for reducing response payload size before filtering.
pub mod field_stripper;
/// Main filtering pipeline orchestrating all stages.
pub mod pipeline;
/// Progressive disclosure annotation for filtered content.
pub mod progressive;
/// Relevance scoring based on keyword overlap.
pub mod relevance;
/// Token counting heuristics.
pub mod token_count;

pub use budget::BudgetEnforcer;
pub use chunker::{Chunk, ChunkSplitter, ChunkStrategy};
pub use content_type::{ContentType, ContentTypeRouter};
pub use dedup::{ChunkHash, DedupFilter, DeliveryLog};
pub use delivery_logger::DeliveryLogger;
pub use embedding_scorer::EmbeddingChunkScorer;
pub use pipeline::{FilterContext, FilterPipeline, FilterResult};
pub use progressive::ProgressiveDisclosureAnnotator;
pub use relevance::RelevanceScorer;
pub use token_count::{count_tokens, measure_response_tokens};
