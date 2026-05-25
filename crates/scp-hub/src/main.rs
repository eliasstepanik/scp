mod admin;
mod extension_tools;
mod hub;
mod listener;
mod metrics;
mod reload;
mod router;
mod server_manager;
mod session_store;
mod streaming;
mod tool_cache;
mod tracing_setup;

use admin::{start_admin_api_with_shutdown, AdminState};
use anyhow::Result;
use clap::Parser;
use listener::{run_stdio_client, ClientListener};
use scp_core::config::load_config;
use scp_filter::pipeline::FilterPipeline;
use scp_index::ToolRegistry;
use scp_pool::PoolManager;
use std::net::SocketAddr;
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

/// Signal handler for graceful shutdown
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, draining in-flight requests...");
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

    // Re-initialize tracing from config if it differs from CLI args
    // This allows config file to override CLI args
    if config.logging.format != (args.log_format) || config.logging.level != args.log_level {
        tracing_setup::init_tracing_from_config(&config.logging);
    }

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

    // Create session store
    let session_store = Arc::new(session_store::SessionStore::new(
        config.hub.defaults.request_token_budget,
    ));

    // Start background cleanup task for expired sessions
    let _cleanup_handle = session_store
        .clone()
        .start_cleanup_task(config.hub.session_timeout_secs);

    // Create filter pipeline
    let filter_pipeline = Arc::new(FilterPipeline::new(&config.filter));

    // Create a shared shutdown signal — created before Router so we can pass a clone in.
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Create router
    let router = Arc::new(router::Router::new(
        pool_manager.clone(),
        tool_registry.clone(),
        config.hub.defaults.fanout_timeout_secs,
        config.hub.defaults.request_token_budget,
        filter_pipeline,
        config.hub.defaults.exposure.clone(),
        config.tool_index.always_include.clone(),
        config.hub.defaults.max_tools_exposed,
        shutdown_tx.clone(),
    ));

    // Start admin API with graceful shutdown
    let admin_state = AdminState {
        server_manager: server_manager.clone(),
        auth_token: config.admin.auth_token.clone(),
        session_store: Some(session_store.clone()),
        tool_registry: Some(tool_registry.clone()),
        config_path: Some(args.config.clone()),
    };

    let admin_listen_addr = config.admin.listen_address.clone();
    let admin_addr = config.admin.port;
    let admin_shutdown_rx = shutdown_rx.resubscribe();
    let admin_handle = tokio::spawn(async move {
        let shutdown = async {
            let mut rx = admin_shutdown_rx;
            let _ = rx.recv().await;
        };
        if let Err(e) =
            start_admin_api_with_shutdown(&admin_listen_addr, admin_addr, admin_state, shutdown)
                .await
        {
            eprintln!("Admin API error: {}", e);
        }
    });

    // Start HTTP client listener with graceful shutdown
    let client_addr = SocketAddr::new(
        config
            .hub
            .listen_address
            .parse()
            .unwrap_or_else(|_| "127.0.0.1".parse().unwrap()),
        config.hub.listen_port,
    );
    let client_listener = ClientListener::new(
        client_addr,
        session_store.clone(),
        router.clone(),
        config.hub.auth.clone(),
    );
    let client_shutdown_rx = shutdown_rx.resubscribe();
    let client_handle = tokio::spawn(async move {
        let shutdown = async {
            let mut rx = client_shutdown_rx;
            let _ = rx.recv().await;
        };
        if let Err(e) = client_listener.run_with_shutdown(shutdown).await {
            eprintln!("HTTP client listener error: {}", e);
        }
    });

    // Start stdio client listener if configured (P2.J)
    let stdio_handle = if config.hub.transports.contains(&"stdio".to_string()) {
        let session_store_clone = session_store.clone();
        let router_clone = router.clone();
        Some(tokio::spawn(async move {
            if let Err(e) = run_stdio_client(session_store_clone, router_clone).await {
                eprintln!("Stdio client error: {}", e);
            }
        }))
    } else {
        None
    };

    info!("SCP Hub started successfully");

    // Eager tool discovery: fire a tools/list fanout in the background so the
    // registry is warm before the first client request arrives.
    {
        let router_clone = router.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            router_clone.discover_tools().await;
        });
    }

    // Wait for shutdown signal
    shutdown_signal().await;

    // Broadcast shutdown signal to all servers
    let _ = shutdown_tx.send(());

    // Wait for all servers to shut down gracefully
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(config.hub.shutdown_timeout_secs),
        async {
            let _ = admin_handle.await;
            let _ = client_handle.await;
            if let Some(handle) = stdio_handle {
                let _ = handle.await;
            }
        },
    )
    .await;

    info!("SCP Hub shutdown complete");

    Ok(())
}
