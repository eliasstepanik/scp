mod admin;
mod hub;
mod reload;
mod router;
mod server_manager;
mod tracing_setup;

use admin::{start_admin_api, AdminState};
use anyhow::Result;
use clap::Parser;
use scp_core::config::load_config;
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Parser)]
#[command(name = "scp-hub")]
#[command(about = "SCP MCP Hub v0.2.0")]
struct Args {
    /// Path to config file
    #[arg(long, default_value = "scp.toml")]
    config: PathBuf,

    /// Log format: json or pretty
    #[arg(long, default_value = "pretty")]
    log_format: String,

    /// Log level: trace, debug, info, warn, error
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Parse log format
    let format = match args.log_format.as_str() {
        "json" => tracing_setup::TracingFormat::Json,
        "pretty" => tracing_setup::TracingFormat::Pretty,
        _ => {
            eprintln!(
                "Invalid log format: {}. Use 'json' or 'pretty'",
                args.log_format
            );
            std::process::exit(1);
        }
    };

    // Initialize tracing
    tracing_setup::init_tracing(format, &args.log_level);

    info!("SCP Hub v0.2.0 starting");

    // Load configuration
    let config = match load_config(&args.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    info!("Config loaded from {:?}", args.config);
    info!(
        "Hub listening on {}:{}",
        config.hub.listen_address, config.hub.listen_port
    );

    // Create pool manager and tool registry
    let pool_manager = Arc::new(PoolManager::new());
    let tool_registry = Arc::new(RwLock::new(ToolRegistry::new()));

    // Create server manager
    let server_manager =
        server_manager::ServerManager::new(pool_manager.clone(), tool_registry.clone());

    // Add servers from config
    for server_config in &config.servers {
        if server_config.enabled {
            match server_manager.add_server(server_config.clone()).await {
                Ok(_) => info!("Server added: {}", server_config.name),
                Err(e) => {
                    eprintln!("Failed to add server {}: {}", server_config.name, e);
                }
            }
        }
    }

    // Start admin API
    let admin_state = AdminState {
        server_manager: server_manager.clone(),
        auth_token: config.admin.auth_token.clone(),
    };

    let admin_addr = config.admin.port;
    tokio::spawn(async move {
        if let Err(e) = start_admin_api("127.0.0.1", admin_addr, admin_state).await {
            eprintln!("Admin API error: {}", e);
        }
    });

    info!("SCP Hub started successfully");

    // Keep the hub running
    tokio::signal::ctrl_c().await?;
    info!("SCP Hub shutting down");

    Ok(())
}
