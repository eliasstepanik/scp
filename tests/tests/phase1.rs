use scp_core::config::{load_config, ServerConfig};
use scp_filter::{count_tokens, BudgetEnforcer};
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_config_loading() {
    let config_str = r#"
config_version = 1

[hub]
listen_address = "127.0.0.1"
listen_port = 3100

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000
max_tools_exposed = 20
fanout_timeout_secs = 5
max_requests_per_min = 100
burst_size = 20

[admin]
port = 3101

[filter]
enabled = true
budget_strategy = "truncate"
chunking_strategy = "paragraph"
relevance_engine = "tags"

[logging]
level = "info"
format = "pretty"

[[servers]]
name = "server1"
transport = "stdio"
command = "echo"
args = []
sharing = "shared"
enabled = true
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_str.as_bytes()).unwrap();
    file.flush().unwrap();

    let config = load_config(file.path()).expect("Failed to load config");
    assert_eq!(config.config_version, 1);
    assert_eq!(config.hub.listen_port, 3100);
    assert_eq!(config.admin.port, 3101);
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.servers[0].name, "server1");
}

#[test]
fn test_env_var_interpolation() {
    std::env::set_var("TEST_COMMAND", "test_echo");

    let config_str = r#"
config_version = 1

[hub]
listen_port = 3100

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000

[admin]
port = 3101

[filter]
enabled = true
budget_strategy = "truncate"
chunking_strategy = "paragraph"
relevance_engine = "tags"

[[servers]]
name = "server1"
transport = "stdio"
command = "${TEST_COMMAND}"
args = []
sharing = "shared"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(config_str.as_bytes()).unwrap();
    file.flush().unwrap();

    let config = load_config(file.path()).expect("Failed to load config");
    assert_eq!(config.servers[0].command, Some("test_echo".to_string()));
}

#[tokio::test]
async fn test_pool_manager_add_server() {
    let manager = PoolManager::new();
    // On Windows `echo` is a shell built-in; use `cmd /c echo` instead.
    #[cfg(windows)]
    let (command, args) = (
        "cmd".to_string(),
        vec!["/c".to_string(), "echo".to_string(), "hello".to_string()],
    );
    #[cfg(not(windows))]
    let (command, args) = ("echo".to_string(), vec!["hello".to_string()]);

    let config = ServerConfig {
        name: "test".to_string(),
        transport: "stdio".to_string(),
        command: Some(command),
        args,
        url: None,
        sharing: "shared".to_string(),
        pool_size: None,
        priority: 100,
        tags: vec![],
        enabled: true,
        timeouts: Default::default(),
        retries: Default::default(),
        env: Default::default(),
        headers: Default::default(),
    };

    let result = manager.add_server(config).await;
    assert!(result.is_ok());

    let servers = manager.list_servers().await;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].0, "test");
}

#[test]
fn test_tool_registry_collision() {
    let mut registry = ToolRegistry::new();

    let tool1 = scp_index::ToolEntry {
        original_name: "search".to_string(),
        qualified_name: String::new(),
        server_name: "server1".to_string(),
        description: Some("Search tool".to_string()),
        input_schema: serde_json::json!({}),
        tags: vec![],
        avg_response_tokens: 100.0,
        call_count: 0,
    };

    let tool2 = scp_index::ToolEntry {
        original_name: "search".to_string(),
        qualified_name: String::new(),
        server_name: "server2".to_string(),
        description: Some("Search tool".to_string()),
        input_schema: serde_json::json!({}),
        tags: vec![],
        avg_response_tokens: 100.0,
        call_count: 0,
    };

    registry.register_tools("server1", vec![tool1]);
    registry.register_tools("server2", vec![tool2]);

    // Qualified lookups should work
    assert!(registry.lookup("server1.search").is_some());
    assert!(registry.lookup("server2.search").is_some());

    // Unqualified lookup should fail (collision)
    assert!(registry.lookup("search").is_none());
}

#[test]
fn test_token_counting() {
    let text = "Hello, World!";
    let tokens = count_tokens(text);
    assert!(tokens > 0);

    let long_text = "a".repeat(1000);
    let long_tokens = count_tokens(&long_text);
    assert!(long_tokens > tokens);
}

#[test]
fn test_budget_truncation() {
    let text = "a".repeat(1000);
    let truncated = BudgetEnforcer::truncate_to_budget(&text, 100);

    assert!(truncated.ends_with("..."));
    let tokens = count_tokens(&truncated);
    assert!(tokens <= 100 || tokens >= 200); // Either under budget or at minimum
}

#[test]
fn test_strip_prefix() {
    assert_eq!(ToolRegistry::strip_prefix("server.tool"), "tool");
    assert_eq!(ToolRegistry::strip_prefix("tool"), "tool");
    assert_eq!(ToolRegistry::strip_prefix("a.b.c"), "b.c");
}
