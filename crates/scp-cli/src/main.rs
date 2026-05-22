use anyhow::Result;
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde_json::json;

const ADMIN_API_URL: &str = "http://127.0.0.1:3101";

#[derive(Parser)]
#[command(name = "scp")]
#[command(about = "SCP Hub CLI v0.2.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the SCP hub
    Start {
        /// Path to config file
        #[arg(long, default_value = "scp.toml")]
        config: String,

        /// Log format: json or pretty
        #[arg(long, default_value = "pretty")]
        log_format: String,

        /// Log level: trace, debug, info, warn, error
        #[arg(long, default_value = "info")]
        log_level: String,
    },

    /// Get hub status
    Status,

    /// Server management commands
    Servers {
        #[command(subcommand)]
        command: ServerCommands,
    },

    /// Reload configuration
    Reload,
}

#[derive(Subcommand)]
enum ServerCommands {
    /// List all servers
    List,

    /// Add a new server
    Add {
        /// Server name
        name: String,

        /// Transport type: stdio, sse, streamable_http
        #[arg(long)]
        transport: String,

        /// Command to run (for stdio transport)
        #[arg(long)]
        command: Option<String>,

        /// URL (for sse/streamable_http transport)
        #[arg(long)]
        url: Option<String>,

        /// Sharing strategy: shared, pooled, dedicated
        #[arg(long, default_value = "shared")]
        sharing: String,
    },

    /// Remove a server
    Remove {
        /// Server name
        name: String,
    },

    /// Disable a server
    Disable {
        /// Server name
        name: String,
    },

    /// Enable a server
    Enable {
        /// Server name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            config,
            log_format,
            log_level,
        } => {
            // Start the hub by spawning scp-hub process
            let mut cmd = std::process::Command::new("scp-hub");
            cmd.arg("--config").arg(&config);
            cmd.arg("--log-format").arg(&log_format);
            cmd.arg("--log-level").arg(&log_level);

            let status = cmd.status()?;
            std::process::exit(status.code().unwrap_or(1));
        }

        Commands::Status => {
            let client = Client::new();
            let response = client
                .get(format!("{}/health", ADMIN_API_URL))
                .send()
                .await?;

            if response.status().is_success() {
                let body = response.json::<serde_json::Value>().await?;
                println!("Hub Status:");
                println!("  Status: {}", body["status"]);
                println!("  Servers: {}", body["servers"]);
                println!("  Healthy: {}", body["healthy"]);
            } else {
                eprintln!("Failed to get hub status: {}", response.status());
            }
        }

        Commands::Servers { command } => match command {
            ServerCommands::List => {
                let client = Client::new();
                let response = client
                    .get(format!("{}/servers", ADMIN_API_URL))
                    .send()
                    .await?;

                if response.status().is_success() {
                    let body = response.json::<serde_json::Value>().await?;
                    println!("Servers:");
                    if let Some(servers) = body["servers"].as_array() {
                        for server in servers {
                            println!(
                                "  {} [{}] - {} tools",
                                server["name"], server["state"], server["tool_count"]
                            );
                        }
                    }
                } else {
                    eprintln!("Failed to list servers: {}", response.status());
                }
            }

            ServerCommands::Add {
                name,
                transport,
                command,
                url,
                sharing,
            } => {
                let client = Client::new();
                let body = json!({
                    "name": name,
                    "transport": transport,
                    "command": command,
                    "url": url,
                    "sharing": sharing,
                    "args": [],
                    "priority": 100,
                    "tags": [],
                    "enabled": true,
                    "timeouts": {},
                    "retries": {},
                    "env": {},
                    "headers": {}
                });

                let response = client
                    .post(format!("{}/servers", ADMIN_API_URL))
                    .json(&body)
                    .send()
                    .await?;

                if response.status().is_success() {
                    println!("Server '{}' added successfully", name);
                } else {
                    eprintln!("Failed to add server: {}", response.status());
                }
            }

            ServerCommands::Remove { name } => {
                let client = Client::new();
                let response = client
                    .delete(format!("{}/servers/{}", ADMIN_API_URL, name))
                    .send()
                    .await?;

                if response.status().is_success() {
                    println!("Server '{}' removed successfully", name);
                } else {
                    eprintln!("Failed to remove server: {}", response.status());
                }
            }

            ServerCommands::Disable { name } => {
                let client = Client::new();
                let response = client
                    .post(format!("{}/servers/{}/disable", ADMIN_API_URL, name))
                    .send()
                    .await?;

                if response.status().is_success() {
                    println!("Server '{}' disabled successfully", name);
                } else {
                    eprintln!("Failed to disable server: {}", response.status());
                }
            }

            ServerCommands::Enable { name } => {
                let client = Client::new();
                let response = client
                    .post(format!("{}/servers/{}/enable", ADMIN_API_URL, name))
                    .send()
                    .await?;

                if response.status().is_success() {
                    println!("Server '{}' enabled successfully", name);
                } else {
                    eprintln!("Failed to enable server: {}", response.status());
                }
            }
        },

        Commands::Reload => {
            let client = Client::new();
            let response = client
                .post(format!("{}/config/reload", ADMIN_API_URL))
                .send()
                .await?;

            if response.status().is_success() {
                println!("Configuration reloaded successfully");
            } else {
                eprintln!("Failed to reload configuration: {}", response.status());
            }
        }
    }

    Ok(())
}
