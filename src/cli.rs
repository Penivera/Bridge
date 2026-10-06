use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use clap::{Parser, Subcommand};

use crate::ipc::{IpcClient, IpcData};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long)]
    /// Path to configuration file (TOML or YAML)
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run the Bridge proxy daemon (default)
    Run {
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Ping the running Bridge daemon via Unix domain socket
    Ping {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Show status of the running Bridge daemon via Unix domain socket
    Status {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// List active domain routes in the running Bridge daemon
    Routes {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Add a route dynamically via Unix domain socket
    AddRoute {
        /// Fully qualified domain name (e.g. app.example.com)
        domain: String,
        #[arg(short, long)]
        /// Upstream socket address (e.g. 127.0.0.1:3000)
        upstream: Option<SocketAddr>,
        #[arg(short, long)]
        /// Target node identifier (e.g. vm-03 or self)
        node_id: Option<String>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Remove a route dynamically via Unix domain socket
    RemoveRoute {
        /// Fully qualified domain name to remove
        domain: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Show cluster membership, leader state, and mesh status
    Cluster {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Inspect domain route targets, hash ring, and session affinity
    Inspect {
        /// Fully qualified domain name to inspect
        domain: String,
        #[arg(short, long)]
        /// Optional client IP to test consistent hash session affinity mapping
        client_ip: Option<IpAddr>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// List active duplicated/replicated service workloads
    Replicas {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Trigger manual failover service duplication for a failed node (presentation/demo)
    Replicate {
        /// Target node identifier that failed (e.g. vm-02)
        #[arg(short, long)]
        node_id: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Trigger manual failback for a duplicated service
    Failback {
        /// Domain name of the duplicated service (e.g. api.example.com)
        domain: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Manage dashboard users (requires the daemon to be running)
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum UserCommands {
    /// Create a new dashboard user
    Create {
        /// Username (letters, digits, '-', '_', '.')
        username: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Change a dashboard user's password
    Passwd {
        /// Existing username
        username: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// List dashboard users
    List {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

/// Outcome of executing a CLI subcommand.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Proceed to run the daemon with the specified configuration file path.
    ContinueWithConfig(Option<PathBuf>),
    /// Terminate process cleanly (client command was handled).
    Exit,
}

/// Executes a CLI command against a running daemon via UDS, or configures daemon run.
pub async fn execute_command(
    command: Commands,
    default_config: Option<&Path>,
) -> Result<CommandOutcome, Box<dyn std::error::Error>> {
    match command {
        Commands::Ping { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let resp = client.ping().await?;
            println!("Bridge IPC ping: {resp}");
            Ok(CommandOutcome::Exit)
        }
        Commands::Status { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let status = client.status().await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(CommandOutcome::Exit)
        }
        Commands::Routes { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let routes = client.list_routes().await?;
            if routes.is_empty() {
                println!("No routes registered.");
            } else {
                println!("{:<32} {:<24} {:<15} {:<8}", "DOMAIN", "UPSTREAM", "NODE_ID", "STATE");
                println!("{:-<82}", "");
                for (domain, info) in routes {
                    let up = info
                        .upstream
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let state = if info.health == "healthy" { "ACTIVE" } else { "DEAD" };
                    println!("{:<32} {:<24} {:<15} {:<8}", domain, up, info.node_id, state);
                }
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::AddRoute {
            domain,
            upstream,
            node_id,
            socket,
        } => {
            let mut client = IpcClient::connect(&socket).await?;
            client.add_route(&domain, upstream, node_id).await?;
            println!("Successfully added route for domain: {domain}");
            Ok(CommandOutcome::Exit)
        }
        Commands::RemoveRoute { domain, socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let removed = client.remove_route(&domain).await?;
            if removed {
                println!("Successfully removed route for domain: {domain}");
            } else {
                println!("Route for domain '{domain}' did not exist.");
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::Cluster { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let status = client.cluster_status().await?;
            if let IpcData::Cluster {
                local_node_id,
                current_leader,
                is_leader,
                peers,
            } = status
            {
                let leader_str = current_leader.unwrap_or_else(|| "None".to_string());
                println!(
                    "LOCAL NODE: {} ({})",
                    local_node_id,
                    if is_leader { "LEADER" } else { "FOLLOWER" }
                );
                println!("CLUSTER LEADER: {}", leader_str);
                println!(
                    "\n{:<16} {:<18} {:<24} {:<12} {:<8}",
                    "NODE ID", "MESH IP", "ENDPOINT", "ROLE", "PRIORITY"
                );
                println!("{:-<80}", "");
                for p in peers {
                    let role = if p.is_leader { "LEADER" } else { "PEER" };
                    let is_self = if p.node_id == local_node_id {
                        " (self)"
                    } else {
                        ""
                    };
                    println!(
                        "{:<16} {:<18} {:<24} {:<12} {:<8}",
                        format!("{}{}", p.node_id, is_self),
                        p.mesh_ip,
                        p.endpoint,
                        role,
                        p.priority
                    );
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::Inspect {
            domain,
            client_ip,
            socket,
        } => {
            let mut client = IpcClient::connect(&socket).await?;
            let inspect = client.inspect_domain(&domain, client_ip).await?;
            if let IpcData::InspectResult {
                domain,
                targets,
                total_targets,
                has_hash_ring,
                client_selected_target,
                client_selected_node_id,
            } = inspect
            {
                println!("DOMAIN: {}", domain);
                println!(
                    "CONSISTENT HASH RING: {}",
                    if has_hash_ring {
                        "Enabled (360 vnodes/node)"
                    } else {
                        "Disabled"
                    }
                );
                println!("TOTAL TARGETS: {}", total_targets);
                println!("\n{:<16} {:<24} {:<24}", "NODE ID", "ADDRESS", "MESH ADDRESS");
                println!("{:-<68}", "");
                for t in targets {
                    let mesh_str = t
                        .mesh_address
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    println!("{:<16} {:<24} {:<24}", t.node_id, t.address, mesh_str);
                }
                if let (Some(target), Some(node_id)) =
                    (client_selected_target, client_selected_node_id)
                {
                    println!("\nCLIENT AFFINITY MAPPING (IP: {}):", client_ip.unwrap());
                    println!("  Selected Node:   {}", node_id);
                    println!("  Target Endpoint: {}", target);
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&inspect)?);
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::Replicas { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let replicas = client.list_replicas().await?;
            if replicas.is_empty() {
                println!("No active service replicas.");
            } else {
                println!(
                    "{:<28} {:<16} {:<16} {:<18} {:<22}",
                    "DOMAIN", "ORIGIN NODE", "ASSIGNED NODE", "FAILBACK MODE", "SPAWNED ADDR"
                );
                println!("{:-<102}", "");
                for r in replicas {
                    println!(
                        "{:<28} {:<16} {:<16} {:<18} {:<22}",
                        r.domain,
                        r.origin_node_id,
                        r.assigned_node_id,
                        r.failback_mode,
                        r.spawned_addr
                    );
                }
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::Replicate { node_id, socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            println!("[DEMO] Triggering failover duplication for node '{node_id}'...");
            let (count, msg) = client.trigger_replication(&node_id).await?;
            println!("SUCCESS: {msg}");
            println!("Spawned {count} duplicate service(s). Traffic now routing to failover instances.");
            Ok(CommandOutcome::Exit)
        }
        Commands::Failback { domain, socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            println!("Triggering failback for domain '{domain}'...");
            let (_restored, msg) = client.failback(&domain).await?;
            println!("SUCCESS: {msg}");
            Ok(CommandOutcome::Exit)
        }
        Commands::User { command } => match command {
            UserCommands::Create { username, socket } => {
                let password = prompt_password(&format!("Password for '{username}': "))?;
                let confirm = prompt_password(&format!("Confirm password for '{username}': "))?;
                if password != confirm {
                    return Err("passwords do not match".into());
                }
                let mut client = IpcClient::connect(&socket).await?;
                let msg = client.create_user(&username, password).await?;
                println!("{msg}");
                Ok(CommandOutcome::Exit)
            }
            UserCommands::Passwd { username, socket } => {
                let password = prompt_password(&format!("New password for '{username}': "))?;
                let confirm = prompt_password(&format!("Confirm new password for '{username}': "))?;
                if password != confirm {
                    return Err("passwords do not match".into());
                }
                let mut client = IpcClient::connect(&socket).await?;
                let msg = client.set_password(&username, password).await?;
                println!("{msg}");
                Ok(CommandOutcome::Exit)
            }
            UserCommands::List { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let users = client.list_users().await?;
                if users.is_empty() {
                    println!("No dashboard users configured.");
                } else {
                    println!("USERS");
                    println!("{:-<32}", "");
                    for u in users {
                        println!("{u}");
                    }
                }
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Run { config: run_cfg } => {
            let selected_config = run_cfg.or_else(|| default_config.map(PathBuf::from));
            Ok(CommandOutcome::ContinueWithConfig(selected_config))
        }
    }
}

/// Reads a password from the terminal without echoing it.
/// Falls back to plain stdin when no TTY is available.
fn prompt_password(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(password) = rpassword::prompt_password(prompt) {
        return Ok(password);
    }
    use std::io::Write;
    let mut stdout = std::io::stdout();
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}
