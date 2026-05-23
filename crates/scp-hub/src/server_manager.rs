use scp_core::config::ServerConfig;
use scp_index::ToolRegistry;
use scp_pool::{PoolManager, ServerState};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

/// Server manager error types.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ManagerError {
    /// Server not found.
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    /// Server already exists.
    #[error("Server already exists: {0}")]
    ServerAlreadyExists(String),

    /// Pool error.
    #[error("Pool error: {0}")]
    PoolError(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Server status information.
#[derive(Debug, Clone)]
pub struct ServerStatus {
    /// Server name.
    pub name: String,
    /// Server state.
    pub state: ServerState,
    /// Number of tools provided by this server.
    pub tool_count: usize,
    /// Whether the server is enabled.
    pub enabled: bool,
}

/// Server manager handles runtime server operations
#[derive(Clone)]
pub struct ServerManager {
    pool_manager: Arc<PoolManager>,
    tool_registry: Arc<RwLock<ToolRegistry>>,
    config: Arc<RwLock<Vec<ServerConfig>>>,
}

impl ServerManager {
    /// Create a new server manager
    pub fn new(pool_manager: Arc<PoolManager>, tool_registry: Arc<RwLock<ToolRegistry>>) -> Self {
        Self {
            pool_manager,
            tool_registry,
            config: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a server at runtime
    pub async fn add_server(&self, config: ServerConfig) -> Result<(), ManagerError> {
        let name = config.name.clone();

        // Add to pool manager
        self.pool_manager
            .add_server(config.clone())
            .await
            .map_err(|e| ManagerError::PoolError(e.to_string()))?;

        // Add to config
        {
            let mut cfg = self.config.write().await;
            cfg.push(config);
        }

        info!("Server added via manager: {}", name);
        Ok(())
    }

    /// Remove a server at runtime
    pub async fn remove_server(&self, name: &str) -> Result<(), ManagerError> {
        // Remove from pool manager
        self.pool_manager
            .remove_server(name)
            .await
            .map_err(|e| ManagerError::PoolError(e.to_string()))?;

        // Remove from tool registry
        {
            let mut registry = self.tool_registry.write().await;
            registry.unregister_server(name);
        }

        // Remove from config
        {
            let mut cfg = self.config.write().await;
            cfg.retain(|s| s.name != name);
        }

        info!("Server removed via manager: {}", name);
        Ok(())
    }

    /// Disable a server
    pub async fn disable_server(&self, name: &str) -> Result<(), ManagerError> {
        self.pool_manager
            .disable_server(name)
            .await
            .map_err(|e| ManagerError::PoolError(e.to_string()))?;

        info!("Server disabled via manager: {}", name);
        Ok(())
    }

    /// Enable a server
    pub async fn enable_server(&self, name: &str) -> Result<(), ManagerError> {
        self.pool_manager
            .enable_server(name)
            .await
            .map_err(|e| ManagerError::PoolError(e.to_string()))?;

        info!("Server enabled via manager: {}", name);
        Ok(())
    }

    /// Update a server's configuration
    pub async fn update_server(
        &self,
        name: &str,
        config: ServerConfig,
    ) -> Result<(), ManagerError> {
        // For now, just update the config
        let mut cfg = self.config.write().await;
        if let Some(pos) = cfg.iter().position(|s| s.name == name) {
            cfg[pos] = config;
            info!("Server updated via manager: {}", name);
            Ok(())
        } else {
            Err(ManagerError::ServerNotFound(name.to_string()))
        }
    }

    /// List all servers with their status
    pub async fn list_servers(&self) -> Vec<ServerStatus> {
        let servers = self.pool_manager.list_servers().await;
        let registry = self.tool_registry.read().await;

        servers
            .into_iter()
            .map(|(name, state)| {
                let tool_count = registry.list_tools_for_server(&name).len();
                ServerStatus {
                    name,
                    state,
                    tool_count,
                    enabled: state != ServerState::Disabled,
                }
            })
            .collect()
    }

    /// Get status of a specific server
    #[allow(dead_code)]
    pub async fn server_status(&self, name: &str) -> Option<ServerStatus> {
        let state = self.pool_manager.get_server_state(name).await.ok()?;
        let registry = self.tool_registry.read().await;
        let tool_count = registry.list_tools_for_server(name).len();

        Some(ServerStatus {
            name: name.to_string(),
            state,
            tool_count,
            enabled: state != ServerState::Disabled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_manager_creation() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let manager = ServerManager::new(pool_manager, tool_registry);

        let servers = manager.list_servers().await;
        assert_eq!(servers.len(), 0);
    }

    #[tokio::test]
    async fn test_add_server() {
        let pool_manager = Arc::new(PoolManager::new());
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let manager = ServerManager::new(pool_manager, tool_registry);

        let config = ServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            command: Some("echo".to_string()),
            args: vec![],
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
        assert_eq!(servers[0].name, "test");
    }
}
