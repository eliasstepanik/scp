use crate::embedding_scorer::EmbeddingToolScorer;
use crate::scorer::ScoringPipeline;
use crate::tfidf::TfIdfIndex;
use crate::usage::UsageTracker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::warn;

/// Tool registry error types
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Tool with the given name was not found in the registry
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Server with the given name was not found in the registry
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    /// Tool name collision detected between multiple servers
    #[error("Collision detected: {0}")]
    Collision(String),
}

/// Tool entry in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Original tool name from server
    pub original_name: String,
    /// Qualified name (server_name.tool_name)
    pub qualified_name: String,
    /// Server that owns this tool
    pub server_name: String,
    /// Tool description
    pub description: Option<String>,
    /// Input schema
    pub input_schema: serde_json::Value,
    /// Tags
    pub tags: Vec<String>,
    /// Average response tokens
    pub avg_response_tokens: f64,
    /// Call count
    pub call_count: u64,
}

/// Tool registry manages all tools from all servers
pub struct ToolRegistry {
    /// qualified_name -> ToolEntry
    tools: HashMap<String, ToolEntry>,
    /// server_name -> [qualified_name]
    server_tools: HashMap<String, Vec<String>>,
    /// original_name -> qualified_name (for unqualified lookups)
    aliases: HashMap<String, String>,
    /// Track collisions: original_name -> [qualified_names]
    collisions: HashMap<String, Vec<String>>,
    /// Usage tracker for per-profile tool call frequency
    pub usage: UsageTracker,
    /// TF-IDF index for scoring tools based on descriptions
    pub tfidf: TfIdfIndex,
    /// Scoring pipeline for ranking tools
    pub scorer: ScoringPipeline,
    /// Optional embedding scorer for embedding-based tool selection
    pub embedding_scorer: Option<EmbeddingToolScorer>,
}

impl ToolRegistry {
    /// Create a new tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            server_tools: HashMap::new(),
            aliases: HashMap::new(),
            collisions: HashMap::new(),
            usage: UsageTracker::new(),
            tfidf: TfIdfIndex::build(&[]),
            scorer: ScoringPipeline::new("tfidf", 0.7, 0.3),
            embedding_scorer: None,
        }
    }

    /// Set the embedding scorer for this registry
    pub fn with_embedding_scorer(mut self, scorer: EmbeddingToolScorer) -> Self {
        self.embedding_scorer = Some(scorer);
        self
    }

    /// Register tools from a server
    pub fn register_tools(&mut self, server: &str, tools: Vec<ToolEntry>) {
        let mut server_tool_list = Vec::new();

        for mut tool in tools {
            let original_name = tool.original_name.clone();
            let qualified_name = format!("{}.{}", server, original_name);

            // Check for collision
            if let Some(existing_qualified) = self.aliases.get(&original_name) {
                // Collision detected
                warn!(
                    "Tool name collision: {} from {} conflicts with {}",
                    original_name, server, existing_qualified
                );

                // Record collision
                self.collisions
                    .entry(original_name.clone())
                    .or_default()
                    .push(qualified_name.clone());

                // Remove unqualified alias since there's a collision
                self.aliases.remove(&original_name);
            } else {
                // No collision yet, add unqualified alias
                self.aliases
                    .insert(original_name.clone(), qualified_name.clone());
            }

            // Update qualified name
            tool.qualified_name = qualified_name.clone();

            // Register the tool
            self.tools.insert(qualified_name.clone(), tool);
            server_tool_list.push(qualified_name);
        }

        // Register server's tool list
        self.server_tools
            .insert(server.to_string(), server_tool_list);

        // Rebuild TF-IDF index with all tools
        let pairs: Vec<(String, Option<String>)> = self
            .tools
            .iter()
            .map(|(name, entry)| (name.clone(), entry.description.clone()))
            .collect();
        self.tfidf = TfIdfIndex::build(&pairs);
    }

    /// Unregister all tools from a server
    pub fn unregister_server(&mut self, server: &str) {
        if let Some(tool_list) = self.server_tools.remove(server) {
            for qualified_name in &tool_list {
                // Remove from tools map
                if let Some(tool) = self.tools.remove(qualified_name) {
                    let original_name = &tool.original_name;

                    // Remove the unqualified alias if it still points at this
                    // (now-removed) qualified name.  Do NOT restore it — the
                    // tool no longer exists in the registry after this call.
                    if self
                        .aliases
                        .get(original_name)
                        .map(|q| q == qualified_name)
                        .unwrap_or(false)
                    {
                        self.aliases.remove(original_name);
                    }

                    // Clean up any collision record that references this qualified name.
                    if let Some(collisions) = self.collisions.get_mut(original_name) {
                        collisions.retain(|q| q != qualified_name);
                        if collisions.is_empty() {
                            self.collisions.remove(original_name);
                        }
                    }
                }
            }

            // If another server's tool now has no collision partner, restore its
            // unqualified alias so it can be looked up without a prefix again.
            for qualified_name in &tool_list {
                // Derive original_name from the qualified_name format "server.original"
                if let Some(dot) = qualified_name.find('.') {
                    let original_name = &qualified_name[dot + 1..];
                    // If no alias exists for this original name, check whether
                    // exactly one remaining server still provides it.
                    if !self.aliases.contains_key(original_name) {
                        let remaining: Vec<&String> = self
                            .server_tools
                            .values()
                            .flat_map(|tools| tools.iter())
                            .filter(|q| {
                                q.find('.')
                                    .map(|d| &q[d + 1..] == original_name)
                                    .unwrap_or(false)
                            })
                            .collect();
                        if remaining.len() == 1 {
                            // Exactly one server still owns this tool → restore alias
                            self.aliases
                                .insert(original_name.to_string(), remaining[0].clone());
                            // Also clear any stale collision entry for it
                            self.collisions.remove(original_name);
                        }
                    }
                }
            }

            // Rebuild TF-IDF index with remaining tools
            let pairs: Vec<(String, Option<String>)> = self
                .tools
                .iter()
                .map(|(name, entry)| (name.clone(), entry.description.clone()))
                .collect();
            self.tfidf = TfIdfIndex::build(&pairs);
        }
    }

    /// Atomically rebuild tools for a server (unregister old, register new, rebuild index)
    pub fn rebuild_for_server(&mut self, server: &str, tools: Vec<ToolEntry>) {
        self.unregister_server(server);
        self.register_tools(server, tools);
    }

    /// Lookup a tool by name (qualified or unqualified)
    pub fn lookup(&self, name: &str) -> Option<&ToolEntry> {
        // Try direct lookup first (qualified name)
        if let Some(tool) = self.tools.get(name) {
            return Some(tool);
        }

        // Try unqualified lookup via alias
        if let Some(qualified_name) = self.aliases.get(name) {
            return self.tools.get(qualified_name);
        }

        None
    }

    /// List all tools (up to max)
    pub fn list_tools(&self, max: usize) -> Vec<&ToolEntry> {
        self.tools.values().take(max).collect()
    }

    /// List tools for a specific server
    pub fn list_tools_for_server(&self, server: &str) -> Vec<&ToolEntry> {
        if let Some(tool_list) = self.server_tools.get(server) {
            tool_list
                .iter()
                .filter_map(|qualified_name| self.tools.get(qualified_name))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Strip server prefix from qualified name
    pub fn strip_prefix(qualified_name: &str) -> &str {
        if let Some(pos) = qualified_name.find('.') {
            &qualified_name[pos + 1..]
        } else {
            qualified_name
        }
    }

    /// Get all tools
    pub fn all_tools(&self) -> Vec<&ToolEntry> {
        self.tools.values().collect()
    }

    /// Get tool count
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get server count
    pub fn server_count(&self) -> usize {
        self.server_tools.len()
    }

    /// Score tools using embedding-based similarity.
    /// Returns a HashMap of qualified_name -> score.
    /// If embedding_scorer is None or query is empty, returns empty HashMap.
    pub async fn score_tools_by_embedding(&self, query: &str) -> HashMap<String, f32> {
        // Return empty if no scorer or empty query
        if self.embedding_scorer.is_none() || query.is_empty() {
            return HashMap::new();
        }

        let scorer = match &self.embedding_scorer {
            Some(s) => s,
            None => return HashMap::new(),
        };

        // Collect all (qualified_name, description) pairs
        let pairs: Vec<(&str, &str)> = self
            .tools
            .iter()
            .map(|(name, entry)| {
                let desc = entry.description.as_deref().unwrap_or("");
                (name.as_str(), desc)
            })
            .collect();

        // Call embedding scorer (returns Vec<(String, f32)>, not Result)
        let results = scorer.score_tools(&pairs, query).await;

        // Convert Vec<(String, f32)> to HashMap<String, f32>
        results.into_iter().collect()
    }

    /// Score, rank, and return top-N tools for a session.
    /// Always-include tools are guaranteed to appear even if they'd be cut by the limit.
    pub fn select_tools(
        &self,
        keywords: &[String],
        profile: &str,
        max: usize,
        always_include: &[String],
        precomputed_scores: Option<&HashMap<String, f32>>,
    ) -> Vec<crate::ScoredTool> {
        // Collect all tools as (qualified_name, entry) pairs
        let tools: Vec<(&str, &ToolEntry)> = self
            .tools
            .iter()
            .map(|(name, entry)| (name.as_str(), entry))
            .collect();

        // Score and rank all tools
        let params = crate::scorer::ScoringParams {
            tools: &tools,
            keywords,
            profile,
            usage: &self.usage,
            tfidf: &self.tfidf,
            always_include,
            precomputed_scores,
        };
        let scored = self.scorer.score_and_rank(params);

        // Take top max tools
        let result: Vec<crate::ScoredTool> = scored.iter().take(max).cloned().collect();

        // Collect names of tools already in result
        let result_names: std::collections::HashSet<String> =
            result.iter().map(|t| t.qualified_name.clone()).collect();

        // Append any always_include tools not already in the top max
        let mut final_result = result;
        for tool_name in always_include {
            if !result_names.contains(tool_name) {
                // Find the tool in scored and add it
                if let Some(tool) = scored.iter().find(|t| &t.qualified_name == tool_name) {
                    final_result.push(tool.clone());
                }
            }
        }

        final_result
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tool(name: &str) -> ToolEntry {
        ToolEntry {
            original_name: name.to_string(),
            qualified_name: String::new(),
            server_name: String::new(),
            description: Some(format!("Test tool: {}", name)),
            input_schema: serde_json::json!({}),
            tags: vec![],
            avg_response_tokens: 100.0,
            call_count: 0,
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.tool_count(), 0);
        assert_eq!(registry.server_count(), 0);
    }

    #[test]
    fn test_register_tools() {
        let mut registry = ToolRegistry::new();
        let tools = vec![create_test_tool("search"), create_test_tool("index")];

        registry.register_tools("server1", tools);

        assert_eq!(registry.tool_count(), 2);
        assert_eq!(registry.server_count(), 1);

        // Check qualified lookup
        assert!(registry.lookup("server1.search").is_some());
        assert!(registry.lookup("server1.index").is_some());

        // Check unqualified lookup (no collision)
        assert!(registry.lookup("search").is_some());
        assert!(registry.lookup("index").is_some());
    }

    #[test]
    fn test_collision_detection() {
        let mut registry = ToolRegistry::new();

        let tools1 = vec![create_test_tool("search")];
        registry.register_tools("server1", tools1);

        let tools2 = vec![create_test_tool("search")];
        registry.register_tools("server2", tools2);

        // Both qualified names should exist
        assert!(registry.lookup("server1.search").is_some());
        assert!(registry.lookup("server2.search").is_some());

        // Unqualified lookup should fail (collision)
        assert!(registry.lookup("search").is_none());
    }

    #[test]
    fn test_unregister_server() {
        let mut registry = ToolRegistry::new();

        let tools = vec![create_test_tool("search")];
        registry.register_tools("server1", tools);

        assert_eq!(registry.tool_count(), 1);

        registry.unregister_server("server1");

        assert_eq!(registry.tool_count(), 0);
        assert_eq!(registry.server_count(), 0);
    }

    #[test]
    fn test_strip_prefix() {
        assert_eq!(ToolRegistry::strip_prefix("server.tool"), "tool");
        assert_eq!(ToolRegistry::strip_prefix("tool"), "tool");
        assert_eq!(ToolRegistry::strip_prefix("a.b.c"), "b.c");
    }

    #[test]
    fn test_list_tools_for_server() {
        let mut registry = ToolRegistry::new();

        let tools1 = vec![create_test_tool("search"), create_test_tool("index")];
        registry.register_tools("server1", tools1);

        let tools2 = vec![create_test_tool("read")];
        registry.register_tools("server2", tools2);

        let server1_tools = registry.list_tools_for_server("server1");
        assert_eq!(server1_tools.len(), 2);

        let server2_tools = registry.list_tools_for_server("server2");
        assert_eq!(server2_tools.len(), 1);
    }

    #[test]
    fn test_select_tools_respects_max_limit() {
        let mut registry = ToolRegistry::new();

        let tools = vec![
            create_test_tool("tool1"),
            create_test_tool("tool2"),
            create_test_tool("tool3"),
            create_test_tool("tool4"),
        ];
        registry.register_tools("server1", tools);

        let keywords = vec![];
        let selected = registry.select_tools(&keywords, "profile1", 2, &[], None);

        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_select_tools_includes_always_include_tools() {
        let mut registry = ToolRegistry::new();

        let tools = vec![
            create_test_tool("tool1"),
            create_test_tool("tool2"),
            create_test_tool("tool3"),
        ];
        registry.register_tools("server1", tools);

        // Record usage for tool1 and tool2 to make them rank higher than tool3
        registry.usage.record_call("profile1", "server1.tool1");
        registry.usage.record_call("profile1", "server1.tool2");

        let keywords = vec![];
        // tool3 has no usage and should not be in top 2
        let always_include = vec!["server1.tool3".to_string()];
        let selected = registry.select_tools(&keywords, "profile1", 2, &always_include, None);

        // tool3 should be in the results
        let has_tool3 = selected.iter().any(|t| t.qualified_name == "server1.tool3");
        assert!(
            has_tool3,
            "tool3 should be in results. Got: {:?}",
            selected
                .iter()
                .map(|t| &t.qualified_name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_rebuild_for_server_replaces_tools() {
        let mut registry = ToolRegistry::new();

        // Register initial tools for server A
        let initial_tools = vec![
            create_test_tool("search"),
            create_test_tool("index"),
            create_test_tool("delete"),
        ];
        registry.register_tools("serverA", initial_tools);
        assert_eq!(registry.tool_count(), 3);

        // Rebuild with new tools (2 new tools, 1 old tool removed)
        let new_tools = vec![create_test_tool("search"), create_test_tool("query")];
        registry.rebuild_for_server("serverA", new_tools);

        // Verify old tools are gone and new ones are present
        assert_eq!(registry.tool_count(), 2);
        assert!(registry.lookup("serverA.search").is_some());
        assert!(registry.lookup("serverA.query").is_some());
        assert!(registry.lookup("serverA.index").is_none());
        assert!(registry.lookup("serverA.delete").is_none());

        // Verify server tools list was updated
        let server_tools = registry.list_tools_for_server("serverA");
        assert_eq!(server_tools.len(), 2);
    }

    #[test]
    fn test_select_tools_respects_max() {
        let mut registry = ToolRegistry::new();

        let tools = vec![
            create_test_tool("tool1"),
            create_test_tool("tool2"),
            create_test_tool("tool3"),
            create_test_tool("tool4"),
            create_test_tool("tool5"),
            create_test_tool("tool6"),
            create_test_tool("tool7"),
            create_test_tool("tool8"),
            create_test_tool("tool9"),
            create_test_tool("tool10"),
        ];
        registry.register_tools("server1", tools);

        let keywords = vec![];
        let selected = registry.select_tools(&keywords, "profile1", 3, &[], None);

        assert_eq!(
            selected.len(),
            3,
            "Should return exactly 3 tools when max=3"
        );
    }
}
