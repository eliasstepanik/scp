use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::warn;

/// Tool registry error types
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Server not found: {0}")]
    ServerNotFound(String),

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
}

impl ToolRegistry {
    /// Create a new tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            server_tools: HashMap::new(),
            aliases: HashMap::new(),
            collisions: HashMap::new(),
        }
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
    }

    /// Unregister all tools from a server
    pub fn unregister_server(&mut self, server: &str) {
        if let Some(tool_list) = self.server_tools.remove(server) {
            for qualified_name in tool_list {
                // Remove from tools map
                if let Some(tool) = self.tools.remove(&qualified_name) {
                    // Check if we need to restore unqualified alias
                    let original_name = &tool.original_name;

                    // Check if there are other servers with this tool
                    let other_servers: Vec<String> = self
                        .server_tools
                        .iter()
                        .filter(|(_, tools)| tools.contains(&qualified_name))
                        .map(|(s, _)| s.clone())
                        .collect();

                    if other_servers.is_empty() {
                        // No other servers have this tool, restore unqualified alias
                        self.aliases
                            .insert(original_name.clone(), qualified_name.clone());

                        // Remove from collisions if present
                        if let Some(collisions) = self.collisions.get_mut(original_name) {
                            collisions.retain(|q| q != &qualified_name);
                            if collisions.is_empty() {
                                self.collisions.remove(original_name);
                            }
                        }
                    }
                }
            }
        }
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
}
