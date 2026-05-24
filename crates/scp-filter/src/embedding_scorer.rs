use crate::chunker::Chunk;
use crate::relevance::RelevanceScorer;
use scp_core::config::EmbeddingConfig;
use scp_index::{EmbeddingCache, EmbeddingClient, EmbeddingError};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Scores chunks using embedding-based cosine similarity against a query.
/// Falls back to keyword overlap scoring on embedding failure.
pub struct EmbeddingChunkScorer {
    client: Arc<EmbeddingClient>,
    cache: Arc<Mutex<EmbeddingCache>>,
}

impl EmbeddingChunkScorer {
    /// Create a new EmbeddingChunkScorer from configuration.
    pub fn new(config: &EmbeddingConfig) -> Self {
        let client = EmbeddingClient::new(&config.endpoint, &config.model);
        Self {
            client: Arc::new(client),
            cache: Arc::new(Mutex::new(EmbeddingCache::new())),
        }
    }

    /// Score chunks by embedding cosine similarity against query string.
    /// Mutates chunk.score in place.
    /// Falls back to RelevanceScorer::score_chunks on embedding failure.
    ///
    /// # Arguments
    /// * `chunks` - Mutable vector of chunks to score
    /// * `query` - Query string to embed and compare against
    /// * `query_terms` - Fallback query terms for keyword scoring
    pub async fn score_chunks(&self, chunks: &mut [Chunk], query: &str, query_terms: &[String]) {
        // If query is empty, use keyword fallback
        if query.is_empty() {
            RelevanceScorer::score_chunks(chunks, query_terms);
            return;
        }

        // If chunks is empty, return immediately
        if chunks.is_empty() {
            return;
        }

        // Try to embed and score using embeddings
        if let Err(e) = self.score_chunks_internal(chunks, query).await {
            log::warn!(
                "Embedding scoring failed, falling back to keyword overlap: {}",
                e
            );
            RelevanceScorer::score_chunks(chunks, query_terms);
        }
    }

    /// Internal implementation of embedding-based scoring.
    async fn score_chunks_internal(
        &self,
        chunks: &mut [Chunk],
        query: &str,
    ) -> Result<(), EmbeddingError> {
        // Embed the query
        let query_embedding = self.embed_text(query).await?;

        // Embed all chunks and compute similarities
        for chunk in chunks.iter_mut() {
            let chunk_embedding = self.embed_text(&chunk.text).await?;
            let similarity = EmbeddingClient::cosine_similarity(&query_embedding, &chunk_embedding);
            chunk.score = similarity;
        }

        Ok(())
    }

    /// Embed a single text, checking cache first.
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        // Check cache
        {
            let cache = self.cache.lock().await;
            if let Some(embedding) = cache.get(text) {
                return Ok(embedding.clone());
            }
        }

        // Embed the text
        let embeddings = self.client.embed(&[text]).await?;
        if embeddings.is_empty() {
            return Err(EmbeddingError::ParseError(
                "No embeddings returned".to_string(),
            ));
        }

        let embedding = embeddings[0].clone();

        // Cache the embedding
        {
            let mut cache = self.cache.lock().await;
            cache.insert(text, embedding.clone());
        }

        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_score_chunks_empty() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingChunkScorer::new(&config);
        let mut chunks = vec![];
        let query_terms = vec!["test".to_string()];

        // Should not panic
        scorer
            .score_chunks(&mut chunks, "test query", &query_terms)
            .await;
    }

    #[tokio::test]
    async fn test_score_chunks_empty_query() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingChunkScorer::new(&config);
        let mut chunks = vec![
            Chunk::new("hello world".to_string(), 0),
            Chunk::new("foo bar".to_string(), 1),
        ];
        let query_terms = vec!["hello".to_string(), "world".to_string()];

        // Empty query should fall back to keyword scoring
        scorer.score_chunks(&mut chunks, "", &query_terms).await;

        // Chunks should have scores from keyword fallback
        assert!(chunks[0].score > 0.0);
        assert!(chunks[1].score >= 0.0);
    }

    #[tokio::test]
    async fn test_score_chunks_fallback_on_unavailable() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingChunkScorer::new(&config);
        let mut chunks = vec![
            Chunk::new("hello world".to_string(), 0),
            Chunk::new("foo bar".to_string(), 1),
        ];
        let query_terms = vec!["hello".to_string(), "world".to_string()];

        // This will fail because the embedding service is not running,
        // but it should fall back to keyword scoring without panicking
        scorer
            .score_chunks(&mut chunks, "test query", &query_terms)
            .await;

        // Chunks should have scores from keyword fallback
        assert!(chunks[0].score > 0.0);
        assert!(chunks[1].score >= 0.0);
    }

    #[tokio::test]
    async fn test_score_chunks_uses_cache() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingChunkScorer::new(&config);

        // Manually insert into cache to test cache retrieval
        let text = "test text";
        let embedding = vec![0.1, 0.2, 0.3];
        {
            let mut cache = scorer.cache.lock().await;
            cache.insert(text, embedding.clone());
        }

        // Verify cache has the entry
        {
            let cache = scorer.cache.lock().await;
            assert!(cache.get(text).is_some());
            assert_eq!(cache.get(text).unwrap(), &embedding);
        }
    }

    #[tokio::test]
    async fn test_fallback_to_keyword_on_unavailable() {
        // Create config with bad endpoint URL (unlikely to have anything listening)
        let mut config = EmbeddingConfig::default();
        config.endpoint = "http://127.0.0.1:19999/api/embed".to_string();

        let scorer = EmbeddingChunkScorer::new(&config);

        // Create 3 chunks with varied text
        let mut chunks = vec![
            Chunk::new("hello world".to_string(), 0),
            Chunk::new("foo bar baz".to_string(), 1),
            Chunk::new("hello there friend".to_string(), 2),
        ];

        // Query terms that match the first and third chunks
        let query_terms = vec!["hello".to_string(), "world".to_string()];

        // Call score_chunks — it should fail to connect and fall back to keyword scoring
        scorer
            .score_chunks(&mut chunks, "hello world query", &query_terms)
            .await;

        // Verify that chunks have scores from keyword fallback
        // The first chunk should have the highest score (contains "hello" and "world")
        assert!(
            chunks[0].score > 0.0,
            "First chunk should have positive score"
        );

        // The third chunk should also have a positive score (contains "hello")
        assert!(
            chunks[2].score > 0.0,
            "Third chunk should have positive score"
        );

        // The second chunk should have zero or lower score (no matching keywords)
        assert!(
            chunks[1].score <= chunks[0].score,
            "First chunk should score higher than second"
        );
        assert!(
            chunks[1].score <= chunks[2].score,
            "First chunk should score higher than or equal to third"
        );
    }
}
