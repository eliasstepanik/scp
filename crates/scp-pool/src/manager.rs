use crate::lifecycle::{LifecycleInfo, ServerState};
use crate::shared::SharedPool;
use scp_core::config::ServerConfig;
use scp_transport::stdio_server::StdioServerTransport;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Pool manager error types
#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Server already exists: {0}")]
    ServerAlreadyExists(String),

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Server is disabled")]
    ServerDisabled,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Server entry in the pool
pub struct ServerEntry {
    pub name: String,
    pub config: ServerConfig,
    pub state: Arc<RwLock<LifecycleInfo>>,
    pub pool: Option<Arc<SharedPool>>,
    pub health_check_handle: Option<JoinHandle<()>>,
}

/// Pool manager manages all server connections
pub struct PoolManager {
    servers: Arc<RwLock<HashMap<String, ServerEntry>>>,
}

impl PoolManager {
    /// Create a new pool manager
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a server to the pool
    pub async fn add_server(&self, config: ServerConfig) -> Result<(), ManagerError> {
        let name = config.name.clone();

        // Check if server already exists
        {
            let servers = self.servers.read().await;
            if servers.contains_key(&name) {
                return Err(ManagerError::ServerAlreadyExists(name));
            }
        }

        // Validate transport
        match config.transport.as_str() {
            "stdio" => {
                if config.command.is_none() {
                    return Err(ManagerError::InvalidConfig(
                        "stdio transport requires command".to_string(),
                    ));
                }
            }
            "sse" | "streamable_http" => {
                if config.url.is_none() {
                    return Err(ManagerError::InvalidConfig(format!(
                        "{} transport requires url",
                        config.transport
                    )));
                }
            }
            _ => {
                return Err(ManagerError::InvalidConfig(format!(
                    "Unknown transport: {}",
                    config.transport
                )));
            }
        }

        // Create server entry
        let entry = ServerEntry {
            name: name.clone(),
            config,
            state: Arc::new(RwLock::new(LifecycleInfo::new())),
            pool: None,
            health_check_handle: None,
        };

        // Add to servers map
        {
            let mut servers = self.servers.write().await;
            servers.insert(name.clone(), entry);
        }

        info!("Server added: {}", name);

        // Start the server
        self.start_server(&name).await?;

        Ok(())
    }

    /// Start a server
    pub async fn start_server(&self, name: &str) -> Result<(), ManagerError> {
        // Read config and current state under a read lock, then release it
        let (config, state_arc) = {
            let servers = self.servers.read().await;
            let entry = servers
                .get(name)
                .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;
            (entry.config.clone(), entry.state.clone())
        };

        // Update state to Starting
        {
            let mut state = state_arc.write().await;
            state.transition_to(ServerState::Starting);
        }

        info!("Starting server: {}", name);

        if config.transport == "stdio" {
            let command = config.command.as_deref().ok_or_else(|| {
                ManagerError::InvalidConfig("stdio transport requires command".to_string())
            })?;

            // Substitute {env:VAR} placeholders in args
            let args: Vec<String> = config
                .args
                .iter()
                .map(|a| substitute_env_placeholders(a))
                .collect();

            // Build env map from config.env, substituting placeholders
            let env: HashMap<String, String> = config
                .env
                .iter()
                .map(|(k, v)| (k.clone(), substitute_env_placeholders(v)))
                .collect();

            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

            match StdioServerTransport::spawn(command, &args_ref, &env).await {
                Ok(transport) => {
                    let pool = Arc::new(SharedPool::new(transport));
                    let pool_for_loop = pool.clone();

                    let receive_handle = tokio::spawn(async move {
                        if let Err(e) = pool_for_loop.receive_loop().await {
                            error!("receive_loop exited: {}", e);
                        }
                    });

                    // Store pool and handle under write lock
                    {
                        let mut servers = self.servers.write().await;
                        if let Some(entry) = servers.get_mut(name) {
                            entry.pool = Some(pool);
                            entry.health_check_handle = Some(receive_handle);
                        }
                    }

                    let mut state = state_arc.write().await;
                    state.transition_to(ServerState::Warm);
                }
                Err(e) => {
                    error!("Failed to spawn stdio server {}: {}", name, e);
                    let mut state = state_arc.write().await;
                    state.transition_to(ServerState::Cold);
                    return Err(ManagerError::TransportError(e.to_string()));
                }
            }
        } else {
            // HTTP backends — no pool needed (transports are created per-request)
            let mut state = state_arc.write().await;
            state.transition_to(ServerState::Warm);
        }

        Ok(())
    }

    /// Stop a server
    pub async fn stop_server(&self, name: &str) -> Result<(), ManagerError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(name)
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;

        // Update state to Draining
        {
            let mut state = entry.state.write().await;
            state.transition_to(ServerState::Draining);
        }

        info!("Stopping server: {}", name);

        // Transition to Cold
        {
            let mut state = entry.state.write().await;
            state.transition_to(ServerState::Cold);
        }

        Ok(())
    }

    /// Get a server's pool
    pub async fn get_pool(&self, name: &str) -> Result<Arc<SharedPool>, ManagerError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(name)
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;

        // Check if server is available
        let state = entry.state.read().await;
        if !state.state.is_available() {
            return Err(ManagerError::ServerDisabled);
        }

        // For now, return error (actual pool creation in P1.B)
        entry
            .pool
            .clone()
            .ok_or_else(|| ManagerError::Internal("Pool not initialized".to_string()))
    }

    /// List all servers with their states
    pub async fn list_servers(&self) -> Vec<(String, ServerState)> {
        let servers = self.servers.read().await;
        let mut result = Vec::new();

        for (name, entry) in servers.iter() {
            let state = entry.state.read().await;
            result.push((name.clone(), state.state));
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Get server state
    pub async fn get_server_state(&self, name: &str) -> Result<ServerState, ManagerError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(name)
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;

        let state = entry.state.read().await;
        Ok(state.state)
    }

    /// Disable a server
    pub async fn disable_server(&self, name: &str) -> Result<(), ManagerError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(name)
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;

        let mut state = entry.state.write().await;
        state.transition_to(ServerState::Disabled);

        info!("Server disabled: {}", name);
        Ok(())
    }

    /// Enable a server
    pub async fn enable_server(&self, name: &str) -> Result<(), ManagerError> {
        let servers = self.servers.read().await;
        let entry = servers
            .get(name)
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))?;

        let mut state = entry.state.write().await;
        state.transition_to(ServerState::Warm);

        info!("Server enabled: {}", name);
        Ok(())
    }

    /// Remove a server
    pub async fn remove_server(&self, name: &str) -> Result<(), ManagerError> {
        // Stop the server first
        self.stop_server(name).await?;

        // Remove from map
        {
            let mut servers = self.servers.write().await;
            servers.remove(name);
        }

        info!("Server removed: {}", name);
        Ok(())
    }

    /// Get a server's config by name.
    pub async fn get_server_config(&self, name: &str) -> Result<ServerConfig, ManagerError> {
        let servers = self.servers.read().await;
        servers
            .get(name)
            .map(|entry| entry.config.clone())
            .ok_or_else(|| ManagerError::ServerNotFound(name.to_string()))
    }

    /// List all servers with their configs and current states.
    pub async fn list_server_configs(&self) -> Vec<(String, ServerConfig, ServerState)> {
        let servers = self.servers.read().await;
        let mut result = Vec::new();
        for (name, entry) in servers.iter() {
            let state = entry.state.read().await;
            result.push((name.clone(), entry.config.clone(), state.state));
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Health check all servers
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let servers = self.servers.read().await;
        let mut result = HashMap::new();

        for (name, entry) in servers.iter() {
            let state = entry.state.read().await;
            result.insert(name.clone(), state.state.is_healthy());
        }

        result
    }
}

impl Default for PoolManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Substitute `{env:VAR}` placeholders with actual environment variable values.
/// Unresolved placeholders are left as-is (no error).
fn substitute_env_placeholders(s: &str) -> String {
    let mut result = s.to_string();
    // Simple scan for {env:VAR_NAME} patterns
    while let Some(start) = result.find("{env:") {
        let rest = &result[start + 5..];
        if let Some(end) = rest.find('}') {
            let var_name = &rest[..end];
            let value = std::env::var(var_name).unwrap_or_default();
            let placeholder = format!("{{env:{}}}", var_name);
            result = result.replacen(&placeholder, &value, 1);
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_manager_creation() {
        let manager = PoolManager::new();
        let servers = manager.list_servers().await;
        assert_eq!(servers.len(), 0);
    }

    #[tokio::test]
    async fn test_add_server() {
        let manager = PoolManager::new();
        // Use a real executable that exists on all platforms.
        // On Windows `echo` is a shell built-in, so we route through cmd.
        #[cfg(windows)]
        let (cmd, args): (&str, Vec<String>) = (
            "cmd",
            vec!["/c".to_string(), "echo".to_string(), "hello".to_string()],
        );
        #[cfg(not(windows))]
        let (cmd, args): (&str, Vec<String>) = ("echo", vec!["hello".to_string()]);

        let config = ServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            command: Some(cmd.to_string()),
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
        assert_eq!(servers[0].1, ServerState::Warm);
    }

    #[tokio::test]
    async fn test_disable_enable_server() {
        let manager = PoolManager::new();
        #[cfg(windows)]
        let (cmd, args): (&str, Vec<String>) = (
            "cmd",
            vec!["/c".to_string(), "echo".to_string(), "hello".to_string()],
        );
        #[cfg(not(windows))]
        let (cmd, args): (&str, Vec<String>) = ("echo", vec!["hello".to_string()]);

        let config = ServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            command: Some(cmd.to_string()),
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

        manager.add_server(config).await.unwrap();

        // Disable
        manager.disable_server("test").await.unwrap();
        let state = manager.get_server_state("test").await.unwrap();
        assert_eq!(state, ServerState::Disabled);

        // Enable
        manager.enable_server("test").await.unwrap();
        let state = manager.get_server_state("test").await.unwrap();
        assert_eq!(state, ServerState::Warm);
    }
}
