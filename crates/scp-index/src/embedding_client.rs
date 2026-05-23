use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur when calling the embedding service
#[derive(Debug, Error)]
pub enum EmbeddingError {
    /// Embedding service is unavailable or returned an error
    #[error("Embedding service unavailable: {0}")]
    Unavailable(String),
    /// Failed to parse the embedding response
    #[error("Embedding response parse error: {0}")]
    ParseError(String),
}

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Client for calling an embedding service (e.g., Ollama, OpenAI)
pub struct EmbeddingClient {
    client: Client,
    endpoint: String,
    model: String,
}

impl EmbeddingClient {
    /// Create a new embedding client with the given endpoint and model name
    pub fn new(endpoint: &str, model: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            client,
            endpoint: endpoint.to_string(),
            model: model.to_string(),
        }
    }

    /// Embed a batch of texts and return their embeddings
    pub async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let body = EmbedRequest {
            model: self.model.clone(),
            input: texts.iter().map(|s| s.to_string()).collect(),
        };
        let response = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbeddingError::Unavailable(e.to_string()))?;

        if !response.status().is_success() {
            return Err(EmbeddingError::Unavailable(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let parsed: EmbedResponse = response
            .json()
            .await
            .map_err(|e| EmbeddingError::ParseError(e.to_string()))?;

        if parsed.embeddings.len() != texts.len() {
            return Err(EmbeddingError::ParseError(format!(
                "Expected {} embeddings, got {}",
                texts.len(),
                parsed.embeddings.len()
            )));
        }

        Ok(parsed.embeddings)
    }

    /// Compute cosine similarity between two vectors
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let vec = vec![1.0, 2.0, 3.0];
        let similarity = EmbeddingClient::cosine_similarity(&vec, &vec);
        assert!((similarity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let similarity = EmbeddingClient::cosine_similarity(&a, &b);
        assert!((similarity - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a = vec![];
        let b = vec![];
        let similarity = EmbeddingClient::cosine_similarity(&a, &b);
        assert_eq!(similarity, 0.0);
    }

    #[test]
    fn test_embed_empty_input() {
        let client = EmbeddingClient::new("http://localhost:11434/api/embed", "test-model");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { client.embed(&[]).await });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Vec::<Vec<f32>>::new());
    }
}
