mod hub;
mod tracing_setup;

use anyhow::Result;
use clap::Parser;
use scp_core::id_map::IdMap;
use scp_transport::stdio_client::StdioClientTransport;
use scp_transport::stdio_server::StdioServerTransport;
use std::collections::HashMap;
use tracing::info;

#[derive(Parser)]
#[command(name = "scp-hub")]
#[command(about = "SCP MCP Passthrough Hub v0.1.0")]
struct Args {
    /// Backend MCP server command
    #[arg(long)]
    server: String,

    /// Additional arguments for the server command
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    server_args: Vec<String>,

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

    info!("SCP Hub v0.1.0 starting");
    info!("Backend server: {} {:?}", args.server, args.server_args);

    // Spawn backend server
    let server_args: Vec<&str> = args.server_args.iter().map(|s| s.as_str()).collect();
    let mut backend =
        StdioServerTransport::spawn(&args.server, &server_args, &HashMap::new()).await?;

    info!("Backend server spawned");

    // Initialize backend
    let init_params = scp_core::mcp_types::InitializeParams {
        protocol_version: "2025-03-26".to_string(),
        capabilities: Default::default(),
        client_info: scp_core::mcp_types::Implementation {
            name: "scp".to_string(),
            version: "0.1.0".to_string(),
        },
    };
    let backend_init_result = hub::initialize_backend(&mut backend, &init_params).await?;
    info!("Backend initialized");

    // Create client transport
    let mut client = StdioClientTransport::new();
    info!("Client transport created");

    // Handle client initialize
    let (_client_id, _client_params) =
        hub::handle_client_initialize(&mut client, &backend_init_result.capabilities).await?;
    info!("Client initialized");

    // Create ID map
    let mut id_map = IdMap::new("default-session".to_string());

    // Run proxy loop
    hub::run_proxy(&mut client, &mut backend, &mut id_map).await?;

    info!("SCP Hub shutting down");
    Ok(())
}
