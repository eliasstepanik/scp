use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Configuration version — must be 1
pub const CONFIG_VERSION: u32 = 1;

/// Configuration error types
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Invalid config version: expected {}, got {}", CONFIG_VERSION, .0)]
    InvalidVersion(u32),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Multiple validation errors: {0:?}")]
    MultipleErrors(Vec<String>),
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub config_version: u32,
    pub hub: HubConfig,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub filter: FilterConfig,
    pub admin: AdminConfig,
    #[serde(default)]
    pub tool_index: ToolIndexConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Hub configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_transports")]
    pub transports: Vec<String>,
    #[serde(default = "default_max_clients")]
    pub max_clients: usize,
    #[serde(default = "default_session_timeout_secs")]
    pub session_timeout_secs: u64,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    pub defaults: HubDefaults,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    /// Maximum response size in bytes before truncation. Default: 1 MiB.
    #[serde(default = "default_max_response_size_bytes")]
    pub max_response_size_bytes: Option<usize>,
}

fn default_max_response_size_bytes() -> Option<usize> {
    Some(1_048_576)
}

fn default_listen_address() -> String {
    "127.0.0.1".to_string()
}

fn default_listen_port() -> u16 {
    3100
}

fn default_transports() -> Vec<String> {
    vec!["stdio".to_string()]
}

fn default_max_clients() -> usize {
    100
}

fn default_session_timeout_secs() -> u64 {
    3600
}

fn default_shutdown_timeout_secs() -> u64 {
    30
}

/// Hub default values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubDefaults {
    #[serde(default = "default_request_token_budget")]
    pub request_token_budget: usize,
    #[serde(default = "default_session_token_budget")]
    pub session_token_budget: usize,
    #[serde(default = "default_max_tools_exposed")]
    pub max_tools_exposed: usize,
    #[serde(default = "default_fanout_timeout_secs")]
    pub fanout_timeout_secs: u64,
    #[serde(default = "default_max_requests_per_min")]
    pub max_requests_per_min: u32,
    #[serde(default = "default_burst_size")]
    pub burst_size: u32,
    #[serde(default)]
    pub exposure: ExposureConfig,
}

fn default_request_token_budget() -> usize {
    // 0 = unconstrained (no token cap). Set to a positive value to enable budget enforcement.
    0
}

fn default_session_token_budget() -> usize {
    32000
}

fn default_max_tools_exposed() -> usize {
    20
}

fn default_fanout_timeout_secs() -> u64 {
    5
}

fn default_max_requests_per_min() -> u32 {
    100
}

fn default_burst_size() -> u32 {
    20
}

impl Default for HubDefaults {
    fn default() -> Self {
        Self {
            request_token_budget: default_request_token_budget(),
            session_token_budget: default_session_token_budget(),
            max_tools_exposed: default_max_tools_exposed(),
            fanout_timeout_secs: default_fanout_timeout_secs(),
            max_requests_per_min: default_max_requests_per_min(),
            burst_size: default_burst_size(),
            exposure: ExposureConfig::default(),
        }
    }
}

/// Controls which backend tools are exposed in `tools/list` responses.
///
/// The effective pinned individual tools come from `ToolIndexConfig::always_include`.
/// `pinned_servers` here controls which entire servers are always-exposed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    /// Server names whose tools are always included in `tools/list`, up to
    /// `hub.defaults.max_tools_exposed`. Listed in order of priority (first = highest).
    #[serde(default)]
    pub pinned_servers: Vec<String>,

    /// When `true`, `tools/call` routes any qualified `server/tool` name even if
    /// the tool was not present in the session's `tools/list` response.
    /// Default: `true` (hidden tools remain callable).
    // TODO(tool-exposure Phase 2): when false, enforce via Session::tool_scope
    #[serde(default = "default_true")]
    pub allow_unlisted_calls: bool,

    /// When `true`, `scp_search` results include the full `inputSchema` for each
    /// matching tool. Increases result size significantly for complex tools.
    /// Default: `false`.
    #[serde(default)]
    pub search_returns_schema: bool,

    /// When `true`, `inputSchema` is replaced with an empty stub in `tools/list`
    /// responses to save LLM context tokens. The LLM can call `scp_schema` to
    /// fetch the full schema on demand before invoking a complex tool.
    /// Default: `false`.
    #[serde(default)]
    pub strip_input_schema: bool,

    /// When set, tool descriptions in `tools/list` wire responses are truncated to
    /// at most this many UTF-8 characters (with a trailing `…`). The full description
    /// is always kept in the registry for `scp_search` relevance scoring.
    /// Default: `None` (no truncation).
    #[serde(default)]
    pub max_description_chars: Option<usize>,
}

fn default_true() -> bool {
    true
}

impl Default for ExposureConfig {
    fn default() -> Self {
        Self {
            pinned_servers: Vec::new(),
            allow_unlisted_calls: true,
            search_returns_schema: false,
            strip_input_schema: false,
            max_description_chars: None,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    /// Optional display prefix used when building qualified tool names.
    /// When set, tools are exposed as `<name_prefix>/<tool>` instead of
    /// `<name>/<tool>`. Useful when two servers share identical tool names
    /// (e.g. `ssh-proxy` and `ssh-cortex` both expose `exec`).
    #[serde(default)]
    pub name_prefix: Option<String>,
    pub transport: String, // "stdio" | "sse" | "streamable_http"
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    /// When `true`, the URL is used as-is without appending `/mcp`.
    /// Needed for servers whose MCP endpoint lives at the root path.
    #[serde(default)]
    pub raw_url: bool,
    pub sharing: String, // "shared" | "pooled" | "dedicated"
    pub pool_size: Option<usize>,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(default)]
    pub retries: RetryConfig,
    #[serde(default, alias = "environment")]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Dot-separated JSON key paths to strip from backend responses before
    /// the filter pipeline runs. Example: ["metadata.managedFields", "metadata.annotations"]
    #[serde(default)]
    pub response_field_strip: Vec<String>,
}

fn default_priority() -> u32 {
    100
}

fn default_enabled() -> bool {
    true
}

/// Timeout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    #[serde(default = "default_connect_secs")]
    pub connect_secs: u64,
    #[serde(default = "default_request_secs")]
    pub request_secs: u64,
    #[serde(default = "default_health_check_secs")]
    pub health_check_secs: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_secs: default_connect_secs(),
            request_secs: default_request_secs(),
            health_check_secs: default_health_check_secs(),
        }
    }
}

fn default_connect_secs() -> u64 {
    10
}

fn default_request_secs() -> u64 {
    30
}

fn default_health_check_secs() -> u64 {
    5
}

/// Retry configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default = "default_backoff_factor")]
    pub backoff_factor: f64,
}

fn default_max_attempts() -> u32 {
    3
}

fn default_initial_delay_ms() -> u64 {
    100
}

fn default_max_delay_ms() -> u64 {
    5000
}

fn default_backoff_factor() -> f64 {
    2.0
}

/// Embedding configuration for relevance scoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_embedding_model")]
    pub model: String,
    #[serde(default = "default_embedding_dimension")]
    pub dimension: usize,
}

fn default_embedding_endpoint() -> String {
    "https://api.openai.com/v1/embeddings".to_string()
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

fn default_embedding_dimension() -> usize {
    1536
}

/// Filter configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default = "default_filter_enabled")]
    pub enabled: bool,
    #[serde(default = "default_budget_strategy")]
    pub budget_strategy: String, // "truncate" | "summarize" | "hybrid"
    #[serde(default = "default_chunking_strategy")]
    pub chunking_strategy: String, // "paragraph" | "line" | "json_element" | "fixed_size"
    #[serde(default = "default_relevance_engine")]
    pub relevance_engine: String, // "tags" | "tfidf" | "embedding"
    #[serde(default = "default_short_circuit_below_tokens")]
    pub short_circuit_below_tokens: usize,
    #[serde(default = "default_progressive_disclosure_enabled")]
    pub progressive_disclosure_enabled: bool,
    #[serde(default = "default_progressive_hint_text")]
    pub progressive_hint_text: String,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
}

fn default_short_circuit_below_tokens() -> usize {
    100
}

fn default_progressive_disclosure_enabled() -> bool {
    true
}

fn default_progressive_hint_text() -> String {
    "[Content truncated for brevity]".to_string()
}

fn default_filter_enabled() -> bool {
    true
}

fn default_budget_strategy() -> String {
    "truncate".to_string()
}

fn default_chunking_strategy() -> String {
    "paragraph".to_string()
}

fn default_relevance_engine() -> String {
    "tags".to_string()
}

/// Admin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    #[serde(default = "default_admin_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_admin_port")]
    pub port: u16,
    pub auth_token: Option<String>,
}

fn default_admin_listen_address() -> String {
    "127.0.0.1".to_string()
}

fn default_admin_port() -> u16 {
    3101
}

/// Tool index configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolIndexConfig {
    #[serde(default = "default_tool_index_engine")]
    pub engine: String, // "tags" | "tfidf" | "embedding"
    #[serde(default = "default_max_tools_per_list")]
    pub max_tools_per_list: usize,
    #[serde(default)]
    pub always_include: Vec<String>,
}

fn default_tool_index_engine() -> String {
    "tags".to_string()
}

fn default_max_tools_per_list() -> usize {
    20
}

/// Logging configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String, // "json" | "pretty"
    pub file: Option<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Authentication method
    pub method: String,
    /// Simple bearer token for hub-wide authentication enforcement.
    /// When set, every request to the hub (except /health and /metrics) must
    /// include `Authorization: Bearer <bearer_token>` or the hub returns 401.
    #[serde(default)]
    pub bearer_token: Option<String>,
    /// Authentication profiles
    pub profiles: std::collections::HashMap<String, AuthProfile>,
}

/// Authentication profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    /// Bearer token for this profile
    pub token: String,
    /// Token budget per request
    pub token_budget_per_request: usize,
    /// Rate limit per minute
    pub rate_limit_per_minute: Option<u32>,
}

impl AuthConfig {
    /// Resolve a profile name from a bearer token.
    ///
    /// Iterates all profiles and returns the name of the first profile whose
    /// `token` field matches the supplied token.
    pub fn resolve_profile(&self, token: &str) -> Option<String> {
        self.profiles
            .iter()
            .find(|(_, profile)| profile.token == token)
            .map(|(name, _)| name.clone())
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

/// Load configuration from a TOML file with environment variable interpolation
pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    // Read file as string
    let content = std::fs::read_to_string(path)?;

    // Apply environment variable interpolation
    let interpolated = interpolate_env_vars(&content)?;

    // Parse TOML
    let config: Config = toml::from_str(&interpolated)?;

    // Validate config version
    if config.config_version != CONFIG_VERSION {
        return Err(ConfigError::InvalidVersion(config.config_version));
    }

    // Validate configuration
    validate_config(&config)?;

    Ok(config)
}

/// Interpolate environment variables in the format ${VAR_NAME}
fn interpolate_env_vars(content: &str) -> Result<String, ConfigError> {
    let re = Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").expect("Invalid regex");

    let mut result = content.to_string();
    for cap in re.captures_iter(content) {
        let var_name = &cap[1];
        let var_value = std::env::var(var_name)
            .map_err(|_| ConfigError::MissingEnvVar(var_name.to_string()))?;
        let placeholder = &cap[0];
        result = result.replace(placeholder, &var_value);
    }

    Ok(result)
}

/// Validate configuration
fn validate_config(config: &Config) -> Result<(), ConfigError> {
    let mut errors = Vec::new();

    // Validate listen port
    if config.hub.listen_port == 0 {
        errors.push("hub.listen_port must be > 0".to_string());
    }

    // Validate admin port != listen port
    if config.admin.port == config.hub.listen_port {
        errors.push("admin.port must differ from hub.listen_port".to_string());
    }

    // Validate server names are unique
    let mut server_names = std::collections::HashSet::new();
    for server in &config.servers {
        if !server_names.insert(&server.name) {
            errors.push(format!("Duplicate server name: {}", server.name));
        }

        // Validate transport
        match server.transport.as_str() {
            "stdio" | "sse" | "streamable_http" => {}
            _ => errors.push(format!(
                "Invalid transport for server {}: {}",
                server.name, server.transport
            )),
        }

        // Validate sharing strategy
        match server.sharing.as_str() {
            "shared" | "pooled" | "dedicated" => {}
            _ => errors.push(format!(
                "Invalid sharing strategy for server {}: {}",
                server.name, server.sharing
            )),
        }

        // Validate stdio servers have command
        if server.transport == "stdio" && server.command.is_none() {
            errors.push(format!(
                "Server {} with stdio transport must have a command",
                server.name
            ));
        }

        // Validate sse/http servers have url
        if (server.transport == "sse" || server.transport == "streamable_http")
            && server.url.is_none()
        {
            errors.push(format!(
                "Server {} with {} transport must have a url",
                server.name, server.transport
            ));
        }
    }

    // Validate budget strategy
    match config.filter.budget_strategy.as_str() {
        "truncate" | "summarize" | "hybrid" => {}
        _ => errors.push(format!(
            "Invalid budget strategy: {}",
            config.filter.budget_strategy
        )),
    }

    // Validate chunking strategy
    match config.filter.chunking_strategy.as_str() {
        "paragraph" | "line" | "json_element" | "fixed_size" => {}
        _ => errors.push(format!(
            "Invalid chunking strategy: {}",
            config.filter.chunking_strategy
        )),
    }

    // Validate relevance engine
    match config.filter.relevance_engine.as_str() {
        "tags" | "tfidf" | "embedding" => {}
        _ => errors.push(format!(
            "Invalid relevance engine: {}",
            config.filter.relevance_engine
        )),
    }

    if !errors.is_empty() {
        if errors.len() == 1 {
            return Err(ConfigError::Validation(errors.into_iter().next().unwrap()));
        } else {
            return Err(ConfigError::MultipleErrors(errors));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
"#;

        let config: Config = toml::from_str(config_str).expect("Failed to parse config");
        assert_eq!(config.config_version, 1);
        assert_eq!(config.hub.listen_port, 3100);
        assert_eq!(config.admin.port, 3101);
    }

    #[test]
    fn test_env_var_interpolation() {
        std::env::set_var("TEST_VAR", "test_value");
        let content = "test = \"${TEST_VAR}\"";
        let result = interpolate_env_vars(content).expect("Failed to interpolate");
        assert_eq!(result, "test = \"test_value\"");
    }

    #[test]
    fn test_missing_env_var() {
        let content = "test = \"${NONEXISTENT_VAR_XYZ}\"";
        let result = interpolate_env_vars(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_auth_toml_parse() {
        let toml_str = r#"config_version = 1

[hub]
listen_address = "127.0.0.1"
listen_port = 3200
transports = ["http"]

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000
max_tools_exposed = 20
fanout_timeout_secs = 5
max_requests_per_min = 100
burst_size = 20

[hub.auth]
method = "bearer"

[hub.auth.profiles.default]
token = "test-token"
token_budget_per_request = 4000
rate_limit_per_minute = 100

[hub.auth.profiles.limited]
token = "limited-token"
token_budget_per_request = 2000
rate_limit_per_minute = 2

[admin]
port = 3201

[filter]
enabled = true
budget_strategy = "truncate"
chunking_strategy = "paragraph"
relevance_engine = "tags"

[logging]
level = "error"
format = "pretty"

[[servers]]
name = "mock"
transport = "stdio"
command = "echo"
args = []
sharing = "shared"
enabled = true
priority = 100

[servers.timeouts]
connect_secs = 10
request_secs = 30
health_check_secs = 5

[servers.retries]
max_attempts = 3
initial_delay_ms = 100
max_delay_ms = 5000
backoff_factor = 2.0
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");
        let auth = config.hub.auth.expect("hub.auth should be Some");
        assert_eq!(auth.method, "bearer");
        assert!(auth.profiles.contains_key("default"));
        assert_eq!(auth.profiles["default"].token, "test-token");
        assert_eq!(
            auth.resolve_profile("test-token"),
            Some("default".to_string())
        );
        assert_eq!(auth.resolve_profile("wrong-token"), None);
    }

    #[test]
    fn test_validation_duplicate_servers() {
        let config_str = r#"
config_version = 1

[hub]
listen_port = 3100

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000

[admin]
port = 3101

[[servers]]
name = "server1"
transport = "stdio"
command = "echo"
args = []
sharing = "shared"

[[servers]]
name = "server1"
transport = "stdio"
command = "echo"
args = []
sharing = "shared"
"#;

        let config: Config = toml::from_str(config_str).expect("Failed to parse");
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_invalid_transport() {
        let config_str = r#"
config_version = 1

[hub]
listen_port = 3100

[hub.defaults]
request_token_budget = 4000
session_token_budget = 32000

[admin]
port = 3101

[[servers]]
name = "server1"
transport = "invalid"
command = "echo"
args = []
sharing = "shared"
"#;

        let config: Config = toml::from_str(config_str).expect("Failed to parse");
        let result = validate_config(&config);
        assert!(result.is_err());
    }
}
