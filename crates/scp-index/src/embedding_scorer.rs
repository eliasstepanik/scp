use crate::embedding_cache::EmbeddingCache;
use crate::embedding_client::{EmbeddingClient, EmbeddingError};
use scp_core::config::EmbeddingConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

/// Scores tools against a query using embedding-based cosine similarity.
pub struct EmbeddingToolScorer {
    client: Arc<EmbeddingClient>,
    cache: Arc<Mutex<EmbeddingCache>>,
}

impl EmbeddingToolScorer {
    /// Create a new EmbeddingToolScorer from configuration.
    pub fn new(config: &EmbeddingConfig) -> Self {
        let client = EmbeddingClient::new(&config.endpoint, &config.model);
        Self {
            client: Arc::new(client),
            cache: Arc::new(Mutex::new(EmbeddingCache::new())),
        }
    }

    /// Score a list of tools against a query string using embedding cosine similarity.
    ///
    /// # Arguments
    /// * `tools` - slice of (qualified_name, description) tuples
    /// * `query` - the query string to score against
    ///
    /// # Returns
    /// Vec of (qualified_name, score) sorted by score descending.
    /// Falls back to empty Vec / 0.0 scores if embedding call fails.
    pub async fn score_tools(&self, tools: &[(&str, &str)], query: &str) -> Vec<(String, f32)> {
        // Handle empty inputs
        if query.is_empty() || tools.is_empty() {
            return Vec::new();
        }

        // Get or compute query embedding
        let query_embedding = match self.get_or_embed_text(query).await {
            Ok(embedding) => embedding,
            Err(e) => {
                warn!("Failed to embed query: {:?}", e);
                return tools
                    .iter()
                    .map(|(name, _)| (name.to_string(), 0.0))
                    .collect();
            }
        };

        // Collect descriptions and check cache
        let descriptions: Vec<&str> = tools.iter().map(|(_, desc)| *desc).collect();
        let mut tool_embeddings = Vec::new();
        let mut uncached_indices = Vec::new();
        let mut uncached_texts = Vec::new();

        let cache = self.cache.lock().await;
        for (idx, desc) in descriptions.iter().enumerate() {
            if let Some(embedding) = cache.get(desc) {
                tool_embeddings.push(Some(embedding.clone()));
            } else {
                tool_embeddings.push(None);
                uncached_indices.push(idx);
                uncached_texts.push(*desc);
            }
        }
        drop(cache);

        // Batch embed uncached descriptions
        if !uncached_texts.is_empty() {
            match self.client.embed(&uncached_texts).await {
                Ok(embeddings) => {
                    let mut cache = self.cache.lock().await;
                    for (i, embedding) in embeddings.into_iter().enumerate() {
                        let idx = uncached_indices[i];
                        tool_embeddings[idx] = Some(embedding.clone());
                        cache.insert(uncached_texts[i], embedding);
                    }
                }
                Err(e) => {
                    warn!("Failed to embed tool descriptions: {:?}", e);
                    return tools
                        .iter()
                        .map(|(name, _)| (name.to_string(), 0.0))
                        .collect();
                }
            }
        }

        // Compute cosine similarity and build results
        let mut results = Vec::new();
        for (i, (qualified_name, _)) in tools.iter().enumerate() {
            let score = if let Some(embedding) = &tool_embeddings[i] {
                EmbeddingClient::cosine_similarity(&query_embedding, embedding)
            } else {
                0.0
            };
            results.push((qualified_name.to_string(), score));
        }

        // Sort by score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        results
    }

    /// Get embedding from cache or compute it.
    async fn get_or_embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let cache = self.cache.lock().await;
        if let Some(embedding) = cache.get(text) {
            return Ok(embedding.clone());
        }
        drop(cache);

        let embeddings = self.client.embed(&[text]).await?;
        if embeddings.is_empty() {
            return Err(EmbeddingError::ParseError(
                "No embedding returned".to_string(),
            ));
        }

        let embedding = embeddings[0].clone();
        let mut cache = self.cache.lock().await;
        cache.insert(text, embedding.clone());

        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_score_tools_empty_query() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingToolScorer::new(&config);

        let tools = vec![("tool1", "description 1"), ("tool2", "description 2")];
        let result = scorer.score_tools(&tools, "").await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_score_tools_empty_tools() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingToolScorer::new(&config);

        let tools: Vec<(&str, &str)> = vec![];
        let result = scorer.score_tools(&tools, "some query").await;

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_score_tools_fallback_on_error() {
        let config = EmbeddingConfig::default();
        let scorer = EmbeddingToolScorer::new(&config);

        let tools = vec![("tool1", "description 1"), ("tool2", "description 2")];
        // This will fail because the embedding service is not running,
        // but we should get a graceful fallback with 0.0 scores
        let result = scorer.score_tools(&tools, "query").await;

        // Should return all tools with 0.0 scores on error
        assert_eq!(result.len(), tools.len());
        for (_, score) in &result {
            assert_eq!(*score, 0.0);
        }
    }

    #[test]
    fn test_score_tools_sorted_by_score() {
        // This test verifies the sorting logic without needing actual embeddings
        // We'll test the sorting by creating mock results
        let mut results = [
            ("tool1".to_string(), 0.5),
            ("tool2".to_string(), 0.9),
            ("tool3".to_string(), 0.3),
        ];

        // Sort by score descending (same logic as in score_tools)
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        assert_eq!(results[0].1, 0.9);
        assert_eq!(results[1].1, 0.5);
        assert_eq!(results[2].1, 0.3);
    }

    #[tokio::test]
    async fn test_fallback_on_unavailable_endpoint() {
        // Create config with bad endpoint URL (unlikely to have anything listening)
        let config = EmbeddingConfig {
            endpoint: "http://127.0.0.1:19999/api/embed".to_string(),
            ..Default::default()
        };

        let scorer = EmbeddingToolScorer::new(&config);

        // Create a list of tool pairs with one clearly related to the query
        let tools = vec![
            (
                "tool.database_query",
                "Execute SQL queries against the database",
            ),
            ("tool.file_read", "Read contents of a file from disk"),
            ("tool.http_request", "Make HTTP requests to external APIs"),
        ];

        // Call score_tools with a query
        let result = scorer.score_tools(&tools, "database").await;

        // Asserts that the result is NOT empty (graceful fallback returns 0.0 scores for all)
        assert!(!result.is_empty(), "Result should not be empty on fallback");

        // All scores should be 0.0 (since fallback returns zeros, not keyword scores)
        for (_, score) in &result {
            assert_eq!(*score, 0.0, "All scores should be 0.0 on embedding failure");
        }

        // Check that the result has the same number of entries as input tools
        assert_eq!(
            result.len(),
            tools.len(),
            "Result should have same number of entries as input tools"
        );
    }
}
