use anyhow::Result;
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde_json::json;

const ADMIN_API_URL: &str = "http://127.0.0.1:3101";

/// SCP Hub CLI application.
#[derive(Parser)]
#[command(name = "scp")]
#[command(about = "SCP Hub CLI v0.2.0")]
struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

/// Top-level commands for the SCP Hub CLI.
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
        /// Server subcommand to execute.
        #[command(subcommand)]
        command: ServerCommands,
    },

    /// Session management commands (P2.L)
    Sessions {
        /// Session subcommand to execute.
        #[command(subcommand)]
        command: SessionCommands,
    },

    /// Tool management commands (P3.K)
    Tools {
        /// Tools subcommand to execute.
        #[command(subcommand)]
        command: ToolsCommands,
    },

    /// View metrics (P6.T8)
    Metrics {
        /// Admin API URL
        #[arg(long, default_value = ADMIN_API_URL)]
        admin_url: String,
    },

    /// View health status (P6.T8)
    Health {
        /// Admin API URL
        #[arg(long, default_value = ADMIN_API_URL)]
        admin_url: String,
    },

    /// Reload configuration
    Reload,
}

/// Server management subcommands.
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

/// Session management subcommands.
#[derive(Subcommand)]
enum SessionCommands {
    /// List all sessions
    List,

    /// Kill a session
    Kill {
        /// Session ID
        id: String,
    },
}

/// Tool management subcommands.
#[derive(Subcommand)]
enum ToolsCommands {
    /// List all tools
    List {
        /// Admin API URL
        #[arg(long, default_value = ADMIN_API_URL)]
        admin_url: String,
    },

    /// Search for tools by keyword
    Search {
        /// Search keyword
        keyword: String,

        /// Admin API URL
        #[arg(long, default_value = ADMIN_API_URL)]
        admin_url: String,
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

        Commands::Sessions { command } => match command {
            SessionCommands::List => {
                let client = Client::new();
                let response = client
                    .get(format!("{}/admin/sessions", ADMIN_API_URL))
                    .send()
                    .await?;

                if response.status().is_success() {
                    let body = response.json::<serde_json::Value>().await?;
                    println!("Sessions:");
                    println!(
                        "{:<40} {:<15} {:<20} {:<20} {:<10}",
                        "Session ID", "Profile", "Budget Remaining", "Last Active", "Tool Calls"
                    );
                    println!("{}", "-".repeat(105));
                    if let Some(sessions) = body["sessions"].as_array() {
                        for session in sessions {
                            let id = session["id"].as_str().unwrap_or("N/A");
                            let profile = session
                                .get("profile")
                                .and_then(|p| p.as_str())
                                .unwrap_or("N/A");
                            let budget = session["budget_remaining"].as_u64().unwrap_or(0);
                            let last_active = session["last_active"].as_str().unwrap_or("N/A");
                            let tool_calls = session["tool_call_count"].as_u64().unwrap_or(0);
                            println!(
                                "{:<40} {:<15} {:<20} {:<20} {:<10}",
                                id, profile, budget, last_active, tool_calls
                            );
                        }
                    }
                } else {
                    eprintln!("Failed to list sessions: {}", response.status());
                }
            }

            SessionCommands::Kill { id } => {
                let client = Client::new();
                let response = client
                    .delete(format!("{}/admin/sessions/{}", ADMIN_API_URL, id))
                    .send()
                    .await?;

                if response.status().is_success() {
                    println!("Session '{}' killed successfully", id);
                } else {
                    eprintln!("Failed to kill session: {}", response.status());
                }
            }
        },

        Commands::Tools { command } => match command {
            ToolsCommands::List { admin_url } => {
                let client = Client::new();
                let response = client.get(format!("{}/tools", admin_url)).send().await?;

                if response.status().is_success() {
                    let tools = response.json::<Vec<serde_json::Value>>().await?;
                    if tools.is_empty() {
                        println!("No tools registered.");
                    } else {
                        println!("Tools:");
                        println!(
                            "{:<40} {:<20} {:<60} {:<10}",
                            "qualified_name", "server", "description", "call_count"
                        );
                        println!("{}", "-".repeat(130));
                        for tool in tools {
                            let qualified_name = tool["qualified_name"].as_str().unwrap_or("N/A");
                            let server = tool["server"].as_str().unwrap_or("N/A");
                            let description = tool["description"].as_str().unwrap_or("");
                            let truncated_desc = if description.len() > 60 {
                                format!("{}...", &description[..57])
                            } else {
                                description.to_string()
                            };
                            let call_count = tool["call_count"].as_u64().unwrap_or(0);
                            println!(
                                "{:<40} {:<20} {:<60} {:<10}",
                                qualified_name, server, truncated_desc, call_count
                            );
                        }
                    }
                } else {
                    eprintln!("Failed to list tools: {}", response.status());
                }
            }

            ToolsCommands::Search { keyword, admin_url } => {
                let client = Client::new();
                let response = client
                    .get(format!(
                        "{}/tools?q={}",
                        admin_url,
                        urlencoding::encode(&keyword)
                    ))
                    .send()
                    .await?;

                if response.status().is_success() {
                    let tools = response.json::<Vec<serde_json::Value>>().await?;
                    if tools.is_empty() {
                        println!("No tools found matching '{}'.", keyword);
                    } else {
                        println!("Tools matching '{}':", keyword);
                        println!(
                            "{:<40} {:<20} {:<60} {:<10}",
                            "qualified_name", "server", "description", "call_count"
                        );
                        println!("{}", "-".repeat(130));
                        for tool in tools {
                            let qualified_name = tool["qualified_name"].as_str().unwrap_or("N/A");
                            let server = tool["server"].as_str().unwrap_or("N/A");
                            let description = tool["description"].as_str().unwrap_or("");
                            let truncated_desc = if description.len() > 60 {
                                format!("{}...", &description[..57])
                            } else {
                                description.to_string()
                            };
                            let call_count = tool["call_count"].as_u64().unwrap_or(0);
                            println!(
                                "{:<40} {:<20} {:<60} {:<10}",
                                qualified_name, server, truncated_desc, call_count
                            );
                        }
                    }
                } else {
                    eprintln!("Failed to search tools: {}", response.status());
                }
            }
        },

        Commands::Metrics { admin_url } => {
            let client = Client::new();
            let response = client
                .get(format!("{}/admin/metrics", admin_url))
                .send()
                .await?;

            if response.status().is_success() {
                let metrics = response.json::<serde_json::Value>().await?;

                println!("{:<30} {:<15}", "Metric", "Value");
                println!("{}", "-".repeat(45));

                // Print simple metrics
                if let Some(val) = metrics.get("tokens_saved_total") {
                    println!("{:<30} {:<15}", "tokens_saved_total", val);
                }
                if let Some(val) = metrics.get("tokens_delivered_total") {
                    println!("{:<30} {:<15}", "tokens_delivered_total", val);
                }
                if let Some(val) = metrics.get("embedding_fallback_total") {
                    println!("{:<30} {:<15}", "embedding_fallback_total", val);
                }
                if let Some(val) = metrics.get("pool_connections_active") {
                    println!("{:<30} {:<15}", "pool_connections_active", val);
                }
                if let Some(val) = metrics.get("inflight_requests") {
                    println!("{:<30} {:<15}", "inflight_requests", val);
                }

                // Print error metrics
                if let Some(errors) = metrics.get("errors_total").and_then(|e| e.as_object()) {
                    for (kind, count) in errors {
                        println!("{:<30} {:<15}", format!("errors.{}", kind), count);
                    }
                }

                // Print request duration metrics
                if let Some(duration) = metrics
                    .get("request_duration_seconds")
                    .and_then(|d| d.as_object())
                {
                    if let Some(count) = duration.get("count") {
                        println!("{:<30} {:<15}", "request_duration_count", count);
                    }
                    if let Some(sum) = duration.get("sum") {
                        println!("{:<30} {:<15}", "request_duration_sum", sum);
                    }
                }
            } else {
                eprintln!("Failed to get metrics: {}", response.status());
            }
        }

        Commands::Health { admin_url } => {
            let client = Client::new();
            let response = client.get(format!("{}/health", admin_url)).send().await?;

            if response.status().is_success() {
                let health = response.json::<serde_json::Value>().await?;

                let status = health
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                let servers = health.get("servers").and_then(|s| s.as_u64()).unwrap_or(0);
                let healthy = health.get("healthy").and_then(|h| h.as_u64()).unwrap_or(0);
                let sessions = health.get("sessions").and_then(|s| s.as_u64()).unwrap_or(0);

                println!(
                    "Status: {}  Servers: {}/{}  Sessions: {}",
                    status, healthy, servers, sessions
                );

                // Exit with appropriate code
                match status {
                    "ok" | "degraded" => std::process::exit(0),
                    _ => std::process::exit(1),
                }
            } else {
                eprintln!("Failed to get health status: {}", response.status());
                std::process::exit(1);
            }
        }

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
