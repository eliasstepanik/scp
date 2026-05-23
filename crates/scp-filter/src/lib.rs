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
/// Token counting heuristics.
pub mod token_count;
/// Deduplication filtering and delivery log tracking.
pub mod dedup;
/// Relevance scoring based on keyword overlap.
pub mod relevance;
/// Progressive disclosure annotation for filtered content.
pub mod progressive;
/// Delivery logging for tracking sent content.
pub mod delivery_logger;
/// Main filtering pipeline orchestrating all stages.
pub mod pipeline;
/// Embedding-based chunk scoring using cosine similarity.
pub mod embedding_scorer;

pub use budget::BudgetEnforcer;
pub use chunker::{Chunk, ChunkSplitter, ChunkStrategy};
pub use content_type::{ContentType, ContentTypeRouter};
pub use token_count::{count_tokens, measure_response_tokens};
pub use dedup::{ChunkHash, DeliveryLog, DedupFilter};
pub use relevance::RelevanceScorer;
pub use progressive::ProgressiveDisclosureAnnotator;
pub use delivery_logger::DeliveryLogger;
pub use pipeline::{FilterPipeline, FilterContext, FilterResult};
pub use embedding_scorer::EmbeddingChunkScorer;
