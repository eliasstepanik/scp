use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::path::Path;
use serde::{Serialize, Deserialize};

/// In-memory embedding cache with optional disk persistence.
/// Keys are SHA-256 hashes of the input text.
#[derive(Default, Serialize, Deserialize)]
pub struct EmbeddingCache {
    cache: HashMap<String, Vec<f32>>,
}

impl EmbeddingCache {
    /// Create a new empty embedding cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn cache_key(text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Retrieve a cached embedding by text
    pub fn get(&self, text: &str) -> Option<&Vec<f32>> {
        self.cache.get(&Self::cache_key(text))
    }

    /// Insert an embedding into the cache
    pub fn insert(&mut self, text: &str, embedding: Vec<f32>) {
        self.cache.insert(Self::cache_key(text), embedding);
    }

    /// Return the number of cached embeddings
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Save the cache to a JSON file
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Load a cache from a JSON file
    pub fn load(path: &Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = EmbeddingCache::new();
        let text = "hello world";
        let embedding = vec![0.1, 0.2, 0.3];

        cache.insert(text, embedding.clone());
        let retrieved = cache.get(text);

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), &embedding);
    }

    #[test]
    fn test_cache_key_deterministic() {
        let text = "test text";
        let key1 = EmbeddingCache::cache_key(text);
        let key2 = EmbeddingCache::cache_key(text);

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_miss() {
        let cache = EmbeddingCache::new();
        let result = cache.get("nonexistent");

        assert!(result.is_none());
    }

    #[test]
    fn test_cache_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("cache.json");

        // Create and save cache
        let mut cache = EmbeddingCache::new();
        cache.insert("text1", vec![0.1, 0.2, 0.3]);
        cache.insert("text2", vec![0.4, 0.5, 0.6]);
        cache.save(&cache_path).unwrap();

        // Load cache
        let loaded_cache = EmbeddingCache::load(&cache_path).unwrap();

        assert_eq!(loaded_cache.len(), 2);
        assert!(loaded_cache.get("text1").is_some());
        assert!(loaded_cache.get("text2").is_some());
        assert_eq!(loaded_cache.get("text1").unwrap(), &vec![0.1, 0.2, 0.3]);
    }
}
