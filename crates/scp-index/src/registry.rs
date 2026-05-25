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
    /// Qualified name (server_name/tool_name)
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
    /// display_qualified_name -> canonical_qualified_name
    ///
    /// Populated when a server has a `name_prefix` that differs from its real
    /// server name.  E.g. `ssh-proxy` with `name_prefix = "proxy"` registers
    /// `proxy/exec → ssh-proxy/exec` here so that `tools/call` for `proxy/exec`
    /// resolves correctly to the `ssh-proxy` backend.
    display_aliases: HashMap<String, String>,
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
            display_aliases: HashMap::new(),
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
        self.register_tools_with_prefix(server, server, tools);
    }

    /// Register tools from a server, optionally suppressing bare-name collision
    /// detection when the server exposes its tools under a distinct display prefix.
    ///
    /// When `display_prefix != server` (i.e. the server has a `name_prefix`
    /// configured), bare-name aliases are **not** registered and collision
    /// detection against bare names is skipped, because the tools will never
    /// be looked up by their bare name — only as `prefix/tool`.
    fn register_tools_with_prefix(
        &mut self,
        server: &str,
        display_prefix: &str,
        tools: Vec<ToolEntry>,
    ) {
        let has_prefix = !display_prefix.is_empty() && display_prefix != server;
        let mut server_tool_list = Vec::new();

        for mut tool in tools {
            let original_name = tool.original_name.clone();
            let qualified_name = format!("{}/{}", server, original_name);

            // Only manage bare-name aliases when the server has no distinct display
            // prefix.  When a prefix is set (e.g. "proxy" for "ssh-proxy"), the
            // tool is never looked up by its bare name, so collision detection
            // between prefixed servers would produce spurious warnings.
            if !has_prefix {
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
            }

            // Update qualified name and server name
            tool.qualified_name = qualified_name.clone();
            tool.server_name = server.to_string();

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
                // Derive original_name from the qualified_name format "server/original"
                if let Some(slash) = qualified_name.find('/') {
                    let original_name = &qualified_name[slash + 1..];
                    // If no alias exists for this original name, check whether
                    // exactly one remaining server still provides it.
                    if !self.aliases.contains_key(original_name) {
                        let remaining: Vec<&String> = self
                            .server_tools
                            .values()
                            .flat_map(|tools| tools.iter())
                            .filter(|q| {
                                q.find('/')
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

    /// Atomically rebuild tools for a server (unregister old, register new, rebuild index).
    ///
    /// `display_prefix` is the prefix under which the server's tools are exposed to
    /// clients (i.e. the `name_prefix` config value, or the server name if none is
    /// set).  When `display_prefix != server`, bare-name collision detection is
    /// suppressed because the tools are only reachable as `prefix/tool`, not by
    /// their bare names.
    pub fn rebuild_for_server(
        &mut self,
        server: &str,
        display_prefix: &str,
        tools: Vec<ToolEntry>,
    ) {
        // Clear any display aliases that pointed at this server's tools
        self.display_aliases
            .retain(|_, canonical| !canonical.starts_with(&format!("{}/", server)));
        self.unregister_server(server);
        self.register_tools_with_prefix(server, display_prefix, tools);
    }

    /// Register display-name aliases for a server that has a `name_prefix`.
    ///
    /// After calling `rebuild_for_server("ssh-proxy", ...)` with `name_prefix = "proxy"`,
    /// call this with `server = "ssh-proxy"` and `display_prefix = "proxy"` to create
    /// `proxy/exec → ssh-proxy/exec` mappings so that `tools/call` for `proxy/exec`
    /// resolves to the correct backend.
    pub fn register_display_aliases(&mut self, server: &str, display_prefix: &str) {
        if display_prefix == server {
            return; // Nothing to do — prefix matches server name already
        }
        if let Some(tool_list) = self.server_tools.get(server) {
            for canonical_qname in tool_list.clone() {
                // canonical_qname is "server/tool"
                if let Some(slash) = canonical_qname.find('/') {
                    let original_name = &canonical_qname[slash + 1..];
                    let display_qname = format!("{}/{}", display_prefix, original_name);
                    self.display_aliases
                        .insert(display_qname, canonical_qname.clone());
                }
            }
        }
    }

    /// Lookup a tool by name (qualified or unqualified)
    pub fn lookup(&self, name: &str) -> Option<&ToolEntry> {
        // Try direct lookup first (canonical qualified name)
        if let Some(tool) = self.tools.get(name) {
            return Some(tool);
        }

        // Try display alias (e.g. "proxy/exec" → "ssh-proxy/exec")
        if let Some(canonical) = self.display_aliases.get(name) {
            return self.tools.get(canonical);
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
        if let Some(pos) = qualified_name.find('/') {
            &qualified_name[pos + 1..]
        } else {
            qualified_name
        }
    }

    /// Get all tools
    pub fn all_tools(&self) -> Vec<&ToolEntry> {
        self.tools.values().collect()
    }

    /// Search tools by query string using TF-IDF scoring.
    ///
    /// Tokenizes the query, scores every tool in the registry, and returns
    /// all tools with a non-zero score sorted highest-first.  Tools with
    /// no description and no matching tokens receive a score of `0.0` and
    /// are excluded from the result.
    pub fn search_tools(&self, query: &str) -> Vec<(f32, &ToolEntry)> {
        use crate::tfidf::tokenize;

        let terms = tokenize(query);
        if terms.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(f32, &ToolEntry)> = self
            .tools
            .values()
            .filter_map(|entry| {
                let score = self.tfidf.score(&entry.qualified_name, &terms);
                if score > 0.0 {
                    Some((score, entry))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Search tools by query string using TF-IDF scoring over name + description.
    ///
    /// Builds an ad-hoc TF-IDF index that incorporates both the tool's qualified
    /// name and its description as the document text, then returns all tools with
    /// a non-zero score sorted highest-first.  The returned entries are cloned so
    /// the caller owns them without holding a borrow on the registry.
    pub fn search_tools_scored(&self, query: &str) -> Vec<(f32, ToolEntry)> {
        use crate::tfidf::tokenize;

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        // Build document text = original_name (with underscores/hyphens replaced by spaces)
        // + qualified_name + description, so that name tokens are searchable.
        let pairs: Vec<(String, String)> = self
            .tools
            .values()
            .map(|entry| {
                let name_tokens = entry.original_name.replace(['_', '-'], " ");
                let text = format!(
                    "{} {} {}",
                    name_tokens,
                    entry.qualified_name,
                    entry.description.as_deref().unwrap_or("")
                );
                (entry.qualified_name.clone(), text)
            })
            .collect();

        let n = pairs.len() as f32;

        // Tokenize each document
        let doc_tokens: Vec<(String, Vec<String>)> = pairs
            .iter()
            .map(|(name, text)| (name.clone(), tokenize(text)))
            .collect();

        // Compute document frequency per term
        let mut df: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for (_, tokens) in &doc_tokens {
            let unique: std::collections::HashSet<&String> = tokens.iter().collect();
            for term in unique {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }

        // Score each tool
        let mut scored: Vec<(f32, ToolEntry)> = doc_tokens
            .iter()
            .filter_map(|(qualified_name, tokens)| {
                if tokens.is_empty() {
                    return None;
                }

                let total = tokens.len() as f32;
                let mut tf_map: std::collections::HashMap<&str, f32> =
                    std::collections::HashMap::new();
                for token in tokens {
                    *tf_map.entry(token.as_str()).or_insert(0.0) += 1.0 / total;
                }

                let score: f32 = query_terms
                    .iter()
                    .map(|term| {
                        let tf = tf_map.get(term.as_str()).copied().unwrap_or(0.0);
                        let doc_freq = df.get(term).copied().unwrap_or(0) as f32;
                        // IDF: ln(N / df); terms absent from corpus get 0
                        let idf = if doc_freq > 0.0 {
                            (n / doc_freq).ln().max(0.0)
                        } else {
                            0.0
                        };
                        tf * idf
                    })
                    .sum();

                if score > 0.0 {
                    let entry = self.tools.get(qualified_name)?.clone();
                    Some((score, entry))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        scored
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
        assert!(registry.lookup("server1/search").is_some());
        assert!(registry.lookup("server1/index").is_some());

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
        assert!(registry.lookup("server1/search").is_some());
        assert!(registry.lookup("server2/search").is_some());

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
        assert_eq!(ToolRegistry::strip_prefix("server/tool"), "tool");
        assert_eq!(ToolRegistry::strip_prefix("tool"), "tool");
        assert_eq!(ToolRegistry::strip_prefix("a/b/c"), "b/c");
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
        registry.usage.record_call("profile1", "server1/tool1");
        registry.usage.record_call("profile1", "server1/tool2");

        let keywords = vec![];
        // tool3 has no usage and should not be in top 2
        let always_include = vec!["server1/tool3".to_string()];
        let selected = registry.select_tools(&keywords, "profile1", 2, &always_include, None);

        // tool3 should be in the results
        let has_tool3 = selected.iter().any(|t| t.qualified_name == "server1/tool3");
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
        registry.rebuild_for_server("serverA", "serverA", new_tools);

        // Verify old tools are gone and new ones are present
        assert_eq!(registry.tool_count(), 2);
        assert!(registry.lookup("serverA/search").is_some());
        assert!(registry.lookup("serverA/query").is_some());
        assert!(registry.lookup("serverA/index").is_none());
        assert!(registry.lookup("serverA/delete").is_none());

        // Verify server tools list was updated
        let server_tools = registry.list_tools_for_server("serverA");
        assert_eq!(server_tools.len(), 2);
    }

    #[test]
    fn test_tfidf_exact_name_match_ranks_first() {
        let mut registry = ToolRegistry::new();

        let mut read_file = create_test_tool("read_file");
        read_file.description = Some("reads a file from the disk".to_string());

        let mut web_search = create_test_tool("web_search");
        web_search.description = Some("queries the internet for information".to_string());

        registry.register_tools("server", vec![read_file, web_search]);

        // "read_file" appears literally in the qualified name of read_file tool
        let results = registry.search_tools_scored("read file");
        assert!(!results.is_empty(), "should return results for 'read file'");
        assert_eq!(
            results[0].1.original_name, "read_file",
            "read_file tool should rank first"
        );
    }

    #[test]
    fn test_tfidf_multi_term_query() {
        let mut registry = ToolRegistry::new();

        let mut alpha = create_test_tool("alpha");
        alpha.description = Some("filesystem read file operations".to_string());

        let mut beta = create_test_tool("beta");
        beta.description = Some("filesystem write operations".to_string());

        let mut gamma = create_test_tool("gamma");
        gamma.description = Some("network query results".to_string());

        registry.register_tools("server", vec![alpha, beta, gamma]);

        // alpha matches "filesystem" + "read", beta matches only "filesystem", gamma matches none
        let results = registry.search_tools_scored("filesystem read");
        assert!(!results.is_empty());

        let names: Vec<&str> = results
            .iter()
            .map(|(_, e)| e.original_name.as_str())
            .collect();
        let pos_alpha = names.iter().position(|&n| n == "alpha");
        let pos_beta = names.iter().position(|&n| n == "beta");

        assert!(pos_alpha.is_some(), "alpha should be in results");
        assert!(pos_beta.is_some(), "beta should be in results");
        assert!(
            pos_alpha.unwrap() < pos_beta.unwrap(),
            "alpha (two matches) should rank above beta (one match)"
        );
    }

    #[test]
    fn test_tfidf_empty_registry_returns_empty() {
        let registry = ToolRegistry::new();
        let results = registry.search_tools_scored("anything");
        assert!(
            results.is_empty(),
            "empty registry should return no results"
        );
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

    #[test]
    fn test_qualified_names_are_prefixed_with_server() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("search_memory"),
            create_test_tool("add_memory"),
        ];
        registry.register_tools("memory-global", tools);

        let entry = registry.lookup("memory-global/search_memory").unwrap();
        assert_eq!(entry.qualified_name, "memory-global/search_memory");
        assert_eq!(entry.server_name, "memory-global");
        assert_eq!(entry.original_name, "search_memory");

        let entry2 = registry.lookup("memory-global/add_memory").unwrap();
        assert_eq!(entry2.qualified_name, "memory-global/add_memory");
    }

    #[test]
    fn test_bare_name_alias_works_when_no_collision() {
        let mut registry = ToolRegistry::new();
        let tools = vec![create_test_tool("search_memory")];
        registry.register_tools("memory-global", tools);

        // Bare name lookup should work when there is no collision
        let entry = registry.lookup("search_memory");
        assert!(
            entry.is_some(),
            "bare name lookup should succeed when no collision"
        );
        assert_eq!(entry.unwrap().qualified_name, "memory-global/search_memory");
    }

    #[test]
    fn test_bare_name_alias_removed_on_collision() {
        let mut registry = ToolRegistry::new();

        let tools1 = vec![create_test_tool("search_memory")];
        registry.register_tools("server-a", tools1);

        let tools2 = vec![create_test_tool("search_memory")];
        registry.register_tools("server-b", tools2);

        // Both qualified names must exist
        assert!(registry.lookup("server-a/search_memory").is_some());
        assert!(registry.lookup("server-b/search_memory").is_some());

        // Bare name must be removed due to collision
        assert!(
            registry.lookup("search_memory").is_none(),
            "bare name lookup must fail when two servers have the same tool name"
        );
    }

    #[test]
    fn test_tools_list_returns_qualified_names() {
        let mut registry = ToolRegistry::new();
        let tools = vec![
            create_test_tool("read_file"),
            create_test_tool("write_file"),
        ];
        registry.register_tools("fs-server", tools);

        let all = registry.all_tools();
        for entry in &all {
            assert!(
                entry.qualified_name.starts_with("fs-server/"),
                "qualified_name '{}' should start with 'fs-server/'",
                entry.qualified_name
            );
            assert!(
                entry.qualified_name.contains('/'),
                "qualified_name should use '/' separator"
            );
        }

        // Verify the exact qualified names
        let names: Vec<&str> = all.iter().map(|e| e.qualified_name.as_str()).collect();
        assert!(names.contains(&"fs-server/read_file"));
        assert!(names.contains(&"fs-server/write_file"));
    }
}
