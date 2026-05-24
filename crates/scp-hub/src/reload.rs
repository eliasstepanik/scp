use scp_core::config::{load_config, Config, ConfigError};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info};

/// Reload error types
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ReloadError {
    #[error("Config error: {0}")]
    /// Configuration file could not be loaded or parsed.
    ConfigError(#[from] ConfigError),

    #[error("Reload already in progress")]
    /// A reload operation is already running; concurrent reloads are not allowed.
    AlreadyReloading,

    #[error("Internal error: {0}")]
    /// An unexpected internal error occurred during reload.
    Internal(String),
}

/// Reload result
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ReloadResult {
    /// Server names that were added by the new configuration.
    pub added: Vec<String>,
    /// Server names that were removed by the new configuration.
    pub removed: Vec<String>,
    /// Server names whose configuration changed.
    pub updated: Vec<String>,
    /// Server names whose configuration is identical to the previous one.
    pub unchanged: Vec<String>,
}

/// Reload manager handles config hot-reload
#[allow(dead_code)]
pub struct ReloadManager {
    config_path: PathBuf,
    current_config: Arc<Mutex<Config>>,
    reloading: Arc<Mutex<bool>>,
}

impl ReloadManager {
    #[allow(dead_code)]
    /// Create a new reload manager
    pub fn new(config_path: PathBuf, initial_config: Config) -> Self {
        Self {
            config_path,
            current_config: Arc::new(Mutex::new(initial_config)),
            reloading: Arc::new(Mutex::new(false)),
        }
    }

    /// Reload configuration
    #[allow(dead_code)]
    pub async fn reload(&self) -> Result<ReloadResult, ReloadError> {
        // Check if already reloading
        {
            let mut reloading = self.reloading.lock().await;
            if *reloading {
                return Err(ReloadError::AlreadyReloading);
            }
            *reloading = true;
        }

        // Load new config
        let new_config = match load_config(&self.config_path) {
            Ok(config) => config,
            Err(e) => {
                let mut reloading = self.reloading.lock().await;
                *reloading = false;
                error!("Failed to load config: {}", e);
                return Err(ReloadError::ConfigError(e));
            }
        };

        // Diff configs
        let current = self.current_config.lock().await;
        let result = diff_configs(&current, &new_config);
        drop(current);

        // Update current config
        {
            let mut current = self.current_config.lock().await;
            *current = new_config;
        }

        // Clear reloading flag
        {
            let mut reloading = self.reloading.lock().await;
            *reloading = false;
        }

        info!("Config reloaded: {:?}", result);
        Ok(result)
    }

    /// Get current config
    #[allow(dead_code)]
    pub async fn get_config(&self) -> Config {
        self.current_config.lock().await.clone()
    }
}

/// Diff two configurations
#[allow(dead_code)]
fn diff_configs(old: &Config, new: &Config) -> ReloadResult {
    let mut result = ReloadResult {
        added: Vec::new(),
        removed: Vec::new(),
        updated: Vec::new(),
        unchanged: Vec::new(),
    };

    let old_names: std::collections::HashSet<_> = old.servers.iter().map(|s| &s.name).collect();
    let new_names: std::collections::HashSet<_> = new.servers.iter().map(|s| &s.name).collect();

    // Find added servers
    for name in new_names.iter() {
        if !old_names.contains(name) {
            result.added.push((*name).clone());
        }
    }

    // Find removed servers
    for name in old_names.iter() {
        if !new_names.contains(name) {
            result.removed.push((*name).clone());
        }
    }

    // Find updated/unchanged servers
    for new_server in &new.servers {
        if let Some(old_server) = old.servers.iter().find(|s| s.name == new_server.name) {
            if old_server.command != new_server.command
                || old_server.url != new_server.url
                || old_server.enabled != new_server.enabled
            {
                result.updated.push(new_server.name.clone());
            } else {
                result.unchanged.push(new_server.name.clone());
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use scp_core::config::{AdminConfig, HubConfig, HubDefaults};

    fn create_test_config(server_count: usize) -> Config {
        let mut servers = Vec::new();
        for i in 0..server_count {
            servers.push(scp_core::config::ServerConfig {
                name: format!("server{}", i),
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
            });
        }

        Config {
            config_version: 1,
            hub: HubConfig {
                listen_address: "127.0.0.1".to_string(),
                listen_port: 3100,
                transports: vec!["stdio".to_string()],
                max_clients: 100,
                session_timeout_secs: 3600,
                defaults: HubDefaults {
                    request_token_budget: 4000,
                    session_token_budget: 32000,
                    max_tools_exposed: 20,
                    fanout_timeout_secs: 5,
                    max_requests_per_min: 100,
                    burst_size: 20,
                },
                auth: None,
            },
            servers,
            filter: Default::default(),
            admin: AdminConfig {
                listen_address: "127.0.0.1".to_string(),
                port: 3101,
                auth_token: None,
            },
            tool_index: Default::default(),
            logging: Default::default(),
        }
    }

    #[test]
    fn test_diff_configs_added() {
        let old = create_test_config(1);
        let new = create_test_config(2);

        let result = diff_configs(&old, &new);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.removed.len(), 0);
    }

    #[test]
    fn test_diff_configs_removed() {
        let old = create_test_config(2);
        let new = create_test_config(1);

        let result = diff_configs(&old, &new);
        assert_eq!(result.added.len(), 0);
        assert_eq!(result.removed.len(), 1);
    }
}
