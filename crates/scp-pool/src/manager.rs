use crate::lifecycle::{LifecycleInfo, ServerState};
use crate::metrics::{SCP_POOL_ACTIVE_PROCESSES, SCP_POOL_SPAWNS_TOTAL};
use crate::shared::SharedPool;
use futures::FutureExt;
use scp_core::config::ServerConfig;
use scp_transport::stdio_server::StdioServerTransport;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

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
                    SCP_POOL_SPAWNS_TOTAL.with_label_values(&[name]).inc();
                    SCP_POOL_ACTIVE_PROCESSES
                        .with_label_values(&[name])
                        .set(1.0);

                    let (pool, receiver) = SharedPool::new(transport);
                    let pool = Arc::new(pool);

                    // Store pool under write lock before spawning the recovery loop
                    {
                        let mut servers = self.servers.write().await;
                        if let Some(entry) = servers.get_mut(name) {
                            entry.pool = Some(pool.clone());
                        }
                    }

                    let name_for_loop = name.to_string();
                    let servers_for_loop = self.servers.clone();

                    // Clones needed to re-spawn inside the recovery loop
                    let command_for_loop = command.to_string();
                    let args_for_loop = args.clone();
                    let env_for_loop = env.clone();
                    let state_for_loop = state_arc.clone();

                    // Clone retry config before moving into spawn
                    let retry_cfg = config.retries.clone();

                    let name_outer = name_for_loop.clone();
                    let state_outer = state_for_loop.clone();

                    let receive_handle = tokio::spawn(
                        std::panic::AssertUnwindSafe(async move {
                            let max_retries = retry_cfg.max_attempts;
                            let mut current_pool = pool;
                            // Wrap in Option so we can move it into receive_loop each
                            // iteration and replenish it after a successful respawn.
                            let mut current_receiver = Some(receiver);

                            loop {
                                // Take the receiver — it is moved into receive_loop so
                                // the loop holds no lock while awaiting stdout.
                                let recv = match current_receiver.take() {
                                    Some(r) => r,
                                    None => {
                                        error!(
                                            server = %name_for_loop,
                                            "receive_loop: no receiver available"
                                        );
                                        break;
                                    }
                                };

                                if let Err(e) = current_pool.receive_loop(recv, &name_for_loop).await {
                                    error!(server = %name_for_loop, "receive_loop exited: {}", e);
                                }

                                // Record the crash against the lifecycle state
                                {
                                    let mut state = state_for_loop.write().await;
                                    state.record_failure("receive_loop exited — server crashed".to_string());
                                }

                                // Check if the server has been administratively disabled/removed
                                {
                                    let servers = servers_for_loop.read().await;
                                    if let Some(entry) = servers.get(&name_for_loop) {
                                        let state = entry.state.read().await;
                                        if matches!(
                                            state.state,
                                            ServerState::Disabled | ServerState::Draining
                                        ) {
                                            // Intentional stop — do not retry
                                            break;
                                        }
                                    } else {
                                        // Server was removed
                                        break;
                                    }
                                }

                                // Attempt crash recovery with exponential backoff
                                let mut restarted = false;
                                for attempt in 1..=max_retries {
                                    let delay_ms = (retry_cfg.initial_delay_ms as f64
                                        * retry_cfg.backoff_factor.powi((attempt - 1) as i32))
                                        .min(retry_cfg.max_delay_ms as f64)
                                        as u64;
                                    let backoff = Duration::from_millis(delay_ms);
                                    warn!(
                                        server = %name_for_loop,
                                        attempt = attempt,
                                        max_attempts = max_retries,
                                        backoff_ms = delay_ms,
                                        "MCP server crashed — retrying"
                                    );
                                    tokio::time::sleep(backoff).await;

                                    let args_ref: Vec<&str> =
                                        args_for_loop.iter().map(|s| s.as_str()).collect();

                                    match StdioServerTransport::spawn(
                                        &command_for_loop,
                                        &args_ref,
                                        &env_for_loop,
                                    )
                                    .await
                                    {
                                        Ok(new_transport) => {
                                            SCP_POOL_SPAWNS_TOTAL
                                                .with_label_values(&[&name_for_loop])
                                                .inc();
                                            SCP_POOL_ACTIVE_PROCESSES
                                                .with_label_values(&[&name_for_loop])
                                                .set(1.0);

                                            let (new_pool, new_receiver) =
                                                SharedPool::new(new_transport);
                                            let new_pool = Arc::new(new_pool);

                                            // Update entry with fresh pool
                                            {
                                                let mut servers = servers_for_loop.write().await;
                                                if let Some(entry) = servers.get_mut(&name_for_loop) {
                                                    entry.pool = Some(new_pool.clone());
                                                }
                                            }

                                            // Record success and restore Warm state
                                            {
                                                let mut state = state_for_loop.write().await;
                                                state.record_success();
                                                state.transition_to(ServerState::Warm);
                                            }

                                            info!(
                                                server = %name_for_loop,
                                                attempt = attempt,
                                                "MCP server recovered after crash"
                                            );

                                            current_pool = new_pool;
                                            current_receiver = Some(new_receiver);
                                            restarted = true;
                                            break;
                                        }
                                        Err(e) => {
                                            {
                                                let mut state = state_for_loop.write().await;
                                                state.record_failure(format!(
                                                    "Spawn attempt {} failed: {}",
                                                    attempt, e
                                                ));
                                            }
                                            error!(
                                                server = %name_for_loop,
                                                attempt = attempt,
                                                max_attempts = max_retries,
                                                "Failed to respawn MCP server: {}", e
                                            );
                                        }
                                    }
                                }

                                if !restarted {
                                    error!(
                                        server = %name_for_loop,
                                        attempts = max_retries,
                                        "MCP server PERMANENTLY FAILED after all retries — server disabled, check logs and config"
                                    );
                                    let mut state = state_for_loop.write().await;
                                    state.transition_to(ServerState::Disabled);
                                    break;
                                }
                            }
                        })
                        .catch_unwind()
                        .map(move |result| {
                            if let Err(panic_val) = result {
                                let msg = panic_val
                                    .downcast_ref::<&str>()
                                    .copied()
                                    .unwrap_or("unknown panic payload");
                                error!(
                                    server = %name_outer,
                                    panic_msg = %msg,
                                    "PANIC in MCP server recovery task — server is now unavailable"
                                );
                                // Note: state_outer cannot be used with .await in a sync context.
                                // The panic disables the recovery loop; the server will remain in its
                                // last state. Operators should restart the hub if this occurs.
                                let _ = state_outer; // keep the clone alive to suppress unused warning
                            }
                        }),
                    );

                    // Store handle under write lock
                    {
                        let mut servers = self.servers.write().await;
                        if let Some(entry) = servers.get_mut(name) {
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
            name_prefix: None,
            transport: "stdio".to_string(),
            command: Some(cmd.to_string()),
            args,
            url: None,
            raw_url: false,
            sharing: "shared".to_string(),
            pool_size: None,
            priority: 100,
            tags: vec![],
            enabled: true,
            timeouts: Default::default(),
            retries: Default::default(),
            env: Default::default(),
            headers: Default::default(),
            response_field_strip: vec![],
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
            name_prefix: None,
            transport: "stdio".to_string(),
            command: Some(cmd.to_string()),
            args,
            url: None,
            raw_url: false,
            sharing: "shared".to_string(),
            pool_size: None,
            priority: 100,
            tags: vec![],
            enabled: true,
            timeouts: Default::default(),
            retries: Default::default(),
            env: Default::default(),
            headers: Default::default(),
            response_field_strip: vec![],
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
