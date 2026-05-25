use scp_index::ToolEntry;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cache entry for tools from a server.
struct CacheEntry {
    /// The cached tools.
    tools: Vec<ToolEntry>,
    /// When the tools were fetched.
    fetched_at: Instant,
}

impl CacheEntry {
    /// Check if this cache entry has expired
    fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

/// Per-server cache for tools with TTL-based expiry.
#[allow(dead_code)]
pub struct ToolCache {
    /// Cached entries by server name.
    entries: HashMap<String, CacheEntry>,
    /// Time-to-live for cache entries.
    ttl: Duration,
}

impl ToolCache {
    /// Create a new tool cache with the specified TTL in seconds
    #[allow(dead_code)]
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Returns cached tools if not expired, None if cache miss or expired
    #[allow(dead_code)]
    pub fn get(&self, server_name: &str) -> Option<Vec<ToolEntry>> {
        if let Some(entry) = self.entries.get(server_name) {
            if !entry.is_expired(self.ttl) {
                return Some(entry.tools.clone());
            }
        }
        None
    }

    /// Stores tools for a server, resets TTL
    #[allow(dead_code)]
    pub fn set(&mut self, server_name: &str, tools: Vec<ToolEntry>) {
        self.entries.insert(
            server_name.to_string(),
            CacheEntry {
                tools,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Invalidates a specific server's cache (called on list_changed notification)
    #[allow(dead_code)]
    pub fn invalidate(&mut self, server_name: &str) {
        self.entries.remove(server_name);
    }

    /// Invalidates all entries
    #[allow(dead_code)]
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let mut cache = ToolCache::new(300);
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server1/test_tool".to_string(),
            server_name: "server1".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];

        cache.set("server1", tools.clone());
        let cached = cache.get("server1");

        assert!(cached.is_some());
        let cached_tools = cached.unwrap();
        assert_eq!(cached_tools.len(), 1);
        assert_eq!(cached_tools[0].original_name, "test_tool");
    }

    #[test]
    fn test_cache_miss_after_ttl() {
        let mut cache = ToolCache::new(0); // 0 second TTL
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server1/test_tool".to_string(),
            server_name: "server1".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];

        cache.set("server1", tools);

        // Sleep briefly to ensure TTL expires
        std::thread::sleep(Duration::from_millis(10));

        let cached = cache.get("server1");
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_invalidate() {
        let mut cache = ToolCache::new(300);
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server1/test_tool".to_string(),
            server_name: "server1".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];

        cache.set("server1", tools);
        assert!(cache.get("server1").is_some());

        cache.invalidate("server1");
        assert!(cache.get("server1").is_none());
    }

    #[test]
    fn test_cache_ttl_not_expired() {
        let mut cache = ToolCache::new(300); // 5 minute TTL
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server1/test_tool".to_string(),
            server_name: "server1".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];

        cache.set("server1", tools);
        let cached = cache.get("server1");

        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn test_cache_invalidate_all() {
        let mut cache = ToolCache::new(300);
        let tools = vec![ToolEntry {
            original_name: "test_tool".to_string(),
            qualified_name: "server1/test_tool".to_string(),
            server_name: "server1".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }];

        cache.set("server1", tools.clone());
        cache.set("server2", tools);

        assert!(cache.get("server1").is_some());
        assert!(cache.get("server2").is_some());

        cache.invalidate_all();

        assert!(cache.get("server1").is_none());
        assert!(cache.get("server2").is_none());
    }
}
