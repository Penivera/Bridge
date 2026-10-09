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
    /// Run comprehensive health checks on the daemon, mesh, and backends
    Health {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Print version, architecture, and feature build info
    Version,
    /// Inspect, validate, or reload daemon configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// List cluster nodes and status (convenience alias)
    Nodes {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Node operations and inspection
    Node {
        #[command(subcommand)]
        command: NodeCommands,
    },
    /// Inspect WireGuard mesh interface and peers
    Mesh {
        #[command(subcommand)]
        command: Option<MeshCommands>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Inspect or trigger leader election
    Election {
        #[command(subcommand)]
        command: Option<ElectionCommands>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Ingress proxy engine status and routing statistics
    Proxy {
        #[command(subcommand)]
        command: Option<ProxyCommands>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// List or prune active domain routes in the running Bridge daemon
    Routes {
        #[command(subcommand)]
        command: Option<RoutesCommands>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Add a route dynamically via Unix domain socket
    #[command(alias = "add_route")]
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
    #[command(alias = "remove_route")]
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
    /// View services auto-discovered by Docker
    Discovery {
        #[command(subcommand)]
        command: Option<DiscoveryCommands>,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
        #[arg(long, default_value = "/var/run/docker.sock")]
        docker_socket: PathBuf,
        /// Force direct Docker scan without querying running daemon
        #[arg(long)]
        local: bool,
        /// Output discovered services as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check for or install Bridge updates from GitHub releases
    Update {
        /// Only check for updates without installing
        #[arg(short, long)]
        check: bool,
        /// Force reinstallation even if already up to date
        #[arg(short, long)]
        force: bool,
        /// Install a specific release tag (e.g. v0.1.0-8f7998b5)
        #[arg(short, long)]
        version: Option<String>,
        /// GitHub repository to fetch releases from
        #[arg(long, default_value = crate::updater::DEFAULT_REPO)]
        repo: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DiscoveryCommands {
    /// List currently discovered Docker services (default)
    List {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
        #[arg(long, default_value = "/var/run/docker.sock")]
        docker_socket: PathBuf,
        #[arg(long)]
        local: bool,
        #[arg(long)]
        json: bool,
    },
    /// Directly scan local Docker containers for routing labels
    Scan {
        #[arg(long, default_value = "/var/run/docker.sock")]
        docker_socket: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommands {
    /// Show active configuration from running daemon or file
    Show {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
        /// Inspect file instead of running daemon
        #[arg(long)]
        file: Option<PathBuf>,
        /// Output pretty-printed JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Validate configuration file syntax and semantics
    Validate {
        /// Configuration file to validate (defaults to auto-discovery)
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Reload configuration file on the running daemon
    Reload {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Get a specific configuration key or section
    Get {
        /// Dotted key path (e.g. proxy.mode, logger.level, node.id)
        key: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Set a configuration key value dynamically
    Set {
        /// Dotted key path (e.g. logger.level, dashboard.public_domain)
        key: String,
        /// New value
        value: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum NodeCommands {
    /// List all nodes participating in the cluster
    List {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Inspect a specific node by ID
    Inspect {
        /// Node ID (e.g. vm-01)
        node_id: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Administratively drop/evict a node from the cluster and mesh
    #[command(alias = "evict", alias = "remove")]
    Drop {
        /// Node ID to drop (e.g. vm-02)
        node_id: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum MeshCommands {
    /// Show WireGuard device status and interface settings
    Status {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// List WireGuard mesh peers and allowed IP routing
    Peers {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Force kernel WireGuard interface and routing synchronization
    #[command(alias = "resync")]
    Sync {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ElectionCommands {
    /// Show current leader election status, term, and candidate roles
    Status {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Trigger an election cycle
    Trigger {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Force specify a cluster leader
    #[command(name = "set-leader", alias = "force", alias = "appoint")]
    SetLeader {
        /// Target node identifier to appoint as leader
        node_id: String,
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Administratively resign/step down from leadership on this node
    #[command(name = "step-down", alias = "resign", alias = "yield")]
    StepDown {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum RoutesCommands {
    /// List active domain routes in the running Bridge daemon
    List {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
    /// Prune dead or orphaned routes targeting evicted/unregistered nodes
    #[command(alias = "clean", alias = "purge")]
    Prune {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProxyCommands {
    /// Show ingress proxy status, mode, and target routing counts
    Status {
        #[arg(short, long, default_value = "/tmp/bridge.sock")]
        socket: PathBuf,
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
        Commands::Health { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let health = client.health().await?;
            if let IpcData::HealthSummary {
                system_health,
                uptime_secs,
                cluster_health,
                local_node_id,
                is_leader,
                total_nodes,
                healthy_nodes,
                total_routes,
                healthy_routes,
                active_replicas,
            } = health
            {
                let status_label = if system_health == "healthy" { "HEALTHY" } else { "DEGRADED" };
                println!("BRIDGE SYSTEM HEALTH: {status_label}");
                println!("{:-<50}", "");
                println!("  Daemon Uptime:    {}", format_uptime(uptime_secs));
                println!("  Cluster Status:   {} ({} node(s))", cluster_health, total_nodes);
                if let Some(nid) = local_node_id {
                    let role = if is_leader == Some(true) { "LEADER" } else { "FOLLOWER" };
                    println!("  Local Node:       {} ({})", nid, role);
                }
                println!("  Healthy Nodes:    {}/{}", healthy_nodes, total_nodes);
                println!("  Active Routes:    {}/{} healthy", healthy_routes, total_routes);
                println!("  Active Replicas:  {}", active_replicas);
            } else {
                println!("{}", serde_json::to_string_pretty(&health)?);
            }
            Ok(CommandOutcome::Exit)
        }
        Commands::Version => {
            println!("bridge {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH);
            println!("Distributed, self-aware, fault-tolerant ingress daemon");
            println!("  Overlay: WireGuard L3 point-to-point mesh");
            println!("  Control: SWIM gossip membership + Bully leader election");
            println!("  Data Plane: L7 Direct HTTP, L4 SNI Passthrough, Managed Coolify");
            Ok(CommandOutcome::Exit)
        }
        Commands::Config { command } => match command {
            ConfigCommands::Show { socket, file, json } => {
                let val = if let Some(path) = file {
                    let raw = std::fs::read_to_string(&path)?;
                    let is_yaml = matches!(path.extension().and_then(|e| e.to_str()), Some("yaml" | "yml"));
                    if is_yaml {
                        serde_yaml::from_str::<serde_json::Value>(&raw)?
                    } else {
                        toml::from_str::<serde_json::Value>(&raw)?
                    }
                } else {
                    let mut client = IpcClient::connect(&socket).await?;
                    let resp = client.get_config().await?;
                    if let IpcData::ConfigView { config, path } = resp {
                        if let Some(p) = path {
                            if !json {
                                println!("# Configuration loaded from: {}", p);
                            }
                        }
                        config
                    } else {
                        return Err("unexpected config response from daemon".into());
                    }
                };

                if json {
                    println!("{}", serde_json::to_string_pretty(&val)?);
                } else {
                    let toml_str = toml::to_string_pretty(&val)?;
                    println!("{}", toml_str);
                }
                Ok(CommandOutcome::Exit)
            }
            ConfigCommands::Validate { config } => {
                let resolved_path = crate::core::config::Config::discover_config_path(
                    config.as_deref().map(std::path::Path::new),
                );
                let mut auto_generated = false;
                let mut pub_key_opt = None;

                if let Some(ref p) = resolved_path {
                    if p.exists() {
                        if let Ok((changed, pk)) =
                            crate::core::config::Config::auto_generate_missing_keys_and_save(p)
                        {
                            auto_generated = changed;
                            pub_key_opt = pk;
                        }
                    }
                }

                match crate::core::config::Config::load_auto(config.as_deref()) {
                    Ok(cfg) => {
                        let path_str = cfg
                            .loaded_from
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "default".to_string());
                        println!("OK: Configuration at '{path_str}' is valid.");
                        if auto_generated {
                            println!("  Auto-generated missing WireGuard keypair and updated '{path_str}'.");
                            if let Some(ref pk) = pub_key_opt {
                                println!("  Generated Public Key: {}", pk);
                            }
                        }
                        println!("  Proxy Mode:    {:?}", cfg.proxy.mode);
                        println!("  Services:      {} registered", cfg.services.len());
                        println!("  Nodes:         {} registered", cfg.nodes.len());
                        if let Some(n) = &cfg.node {
                            println!("  Local Mesh IP: {} (ID: {})", n.mesh_ip, n.id);
                            if let Some(pk) = &n.public_key {
                                println!("  Public Key:    {}", pk);
                            }
                        } else {
                            println!("  Cluster Mesh:  Standalone (no local node configured)");
                        }
                        Ok(CommandOutcome::Exit)
                    }
                    Err(err) => {
                        eprintln!("ERROR: Invalid configuration: {err}");
                        Err(err)
                    }
                }
            }
            ConfigCommands::Reload { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let msg = client.reload_config().await?;
                println!("SUCCESS: {msg}");
                Ok(CommandOutcome::Exit)
            }
            ConfigCommands::Get { key, socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.get_config().await?;
                if let IpcData::ConfigView { config, .. } = resp {
                    let parts: Vec<&str> = key.split('.').collect();
                    let mut cur = &config;
                    for part in parts {
                        cur = &cur[part];
                    }
                    if cur.is_null() {
                        println!("Key '{key}' not found or null");
                    } else if let Some(s) = cur.as_str() {
                        println!("{s}");
                    } else {
                        println!("{}", serde_json::to_string_pretty(cur)?);
                    }
                } else {
                    println!("Unable to read configuration view");
                }
                Ok(CommandOutcome::Exit)
            }
            ConfigCommands::Set { key, value, socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let msg = client.set_config_key(&key, &value).await?;
                println!("SUCCESS: {msg}");
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Nodes { socket } => {
            let mut client = IpcClient::connect(&socket).await?;
            let status = client.cluster_status().await?;
            print_cluster_nodes(&status)?;
            Ok(CommandOutcome::Exit)
        }
        Commands::Node { command } => match command {
            NodeCommands::List { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let status = client.cluster_status().await?;
                print_cluster_nodes(&status)?;
                Ok(CommandOutcome::Exit)
            }
            NodeCommands::Inspect { node_id, socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let status = client.cluster_status().await?;
                if let IpcData::Cluster {
                    local_node_id,
                    current_leader,
                    peers,
                    ..
                } = status
                {
                    if let Some(peer) = peers.iter().find(|p| p.node_id == node_id) {
                        let is_self = peer.node_id == local_node_id;
                        let is_lead = current_leader.as_deref() == Some(&peer.node_id);
                        println!("NODE: {}{}", peer.node_id, if is_self { " (self)" } else { "" });
                        println!("{:-<40}", "");
                        println!("  Mesh IP:    {}", peer.mesh_ip);
                        println!("  Endpoint:   {}", peer.endpoint);
                        println!("  Role:       {}", if is_lead { "LEADER" } else { "PEER" });
                        println!("  Priority:   {}", peer.priority);

                        let routes = client.list_routes().await?;
                        let hosted: Vec<_> = routes.into_iter().filter(|(_, r)| r.node_id == node_id).collect();
                        println!("  Hosted Routes ({}):", hosted.len());
                        for (dom, r) in hosted {
                            println!("    - {:<28} (upstream: {:?}, state: {})", dom, r.upstream, r.health);
                        }
                    } else {
                        println!("Node '{node_id}' not found in cluster.");
                    }
                }
                Ok(CommandOutcome::Exit)
            }
            NodeCommands::Drop { node_id, socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.drop_node(node_id).await?;
                if let IpcData::AdminResult { message, .. } = resp {
                    println!("SUCCESS: {message}");
                } else {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Mesh { command, socket } => match command.unwrap_or(MeshCommands::Status { socket: socket.clone() }) {
            MeshCommands::Status { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let mesh = client.mesh_status().await?;
                if let IpcData::MeshDevice {
                    interface_name,
                    mesh_ip,
                    public_key,
                    listen_port,
                    peers_count,
                    peers,
                } = mesh
                {
                    println!("WIREGUARD L3 OVERLAY MESH");
                    println!("{:-<60}", "");
                    println!("  Interface:        {}", interface_name);
                    println!("  Mesh IP:          {}", mesh_ip);
                    println!("  Public Key:       {}", public_key);
                    println!("  Listen Port:      {}", listen_port);
                    println!("  Connected Peers:  {}", peers_count);
                    if !peers.is_empty() {
                        println!("\n{:<46} {:<24} {:<16}", "PEER PUBLIC KEY", "ENDPOINT", "ALLOWED IPS");
                        println!("{:-<86}", "");
                        for p in peers {
                            let ep = p.endpoint.map(|e| e.to_string()).unwrap_or_else(|| "-".to_string());
                            let ips = p.allowed_ips.join(", ");
                            println!("{:<46} {:<24} {:<16}", p.public_key, ep, ips);
                        }
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(&mesh)?);
                }
                Ok(CommandOutcome::Exit)
            }
            MeshCommands::Peers { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let mesh = client.mesh_status().await?;
                if let IpcData::MeshDevice { peers, .. } = mesh {
                    if peers.is_empty() {
                        println!("No WireGuard peers connected.");
                    } else {
                        println!("{:<46} {:<24} {:<16} {:<10}", "PEER PUBLIC KEY", "ENDPOINT", "ALLOWED IPS", "KEEPALIVE");
                        println!("{:-<96}", "");
                        for p in peers {
                            let ep = p.endpoint.map(|e| e.to_string()).unwrap_or_else(|| "-".to_string());
                            let ips = p.allowed_ips.join(", ");
                            println!("{:<46} {:<24} {:<16} {}s", p.public_key, ep, ips, p.persistent_keepalive);
                        }
                    }
                }
                Ok(CommandOutcome::Exit)
            }
            MeshCommands::Sync { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.mesh_sync().await?;
                if let IpcData::MeshSyncResult { message, .. } = resp {
                    println!("SUCCESS: {message}");
                } else {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Election { command, socket } => match command.unwrap_or(ElectionCommands::Status { socket: socket.clone() }) {
            ElectionCommands::Status { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let info = client.election_status().await?;
                if let IpcData::ElectionInfo {
                    current_leader,
                    term,
                    role,
                    quorum_required,
                    total_known_nodes,
                    acks_count,
                    is_leader,
                } = info
                {
                    println!("BULLY LEADER ELECTION");
                    println!("{:-<50}", "");
                    let lead_str = current_leader.unwrap_or_else(|| "None (Electing)".to_string());
                    println!("  Current Leader:     {} {}", lead_str, if is_leader { "(self)" } else { "" });
                    println!("  Election Term:      {}", term);
                    println!("  Local Role:         {}", role);
                    println!("  Total Known Nodes:  {}", total_known_nodes);
                    println!("  Quorum Required:    {}", quorum_required);
                    println!("  Coordinator Acks:   {}", acks_count);
                } else {
                    println!("{}", serde_json::to_string_pretty(&info)?);
                }
                Ok(CommandOutcome::Exit)
            }
            ElectionCommands::Trigger { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let msg = client.trigger_election().await?;
                println!("SUCCESS: {msg}");
                Ok(CommandOutcome::Exit)
            }
            ElectionCommands::SetLeader { node_id, socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.set_leader(node_id).await?;
                if let IpcData::AdminResult { message, .. } = resp {
                    println!("SUCCESS: {message}");
                } else {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
                Ok(CommandOutcome::Exit)
            }
            ElectionCommands::StepDown { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.step_down().await?;
                if let IpcData::AdminResult { message, .. } = resp {
                    println!("SUCCESS: {message}");
                } else {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Proxy { command, socket } => match command.unwrap_or(ProxyCommands::Status { socket: socket.clone() }) {
            ProxyCommands::Status { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let p = client.proxy_status().await?;
                if let IpcData::ProxyStatus {
                    mode,
                    routes_count,
                    healthy_routes,
                    dead_routes,
                    uptime_secs,
                } = p
                {
                    println!("INGRESS PROXY ENGINE");
                    println!("{:-<50}", "");
                    println!("  Proxy Mode:       {}", mode);
                    println!("  Uptime:           {}", format_uptime(uptime_secs));
                    println!("  Total Routes:     {}", routes_count);
                    println!("  Healthy Routes:   {}", healthy_routes);
                    println!("  Dead Routes:      {}", dead_routes);
                } else {
                    println!("{}", serde_json::to_string_pretty(&p)?);
                }
                Ok(CommandOutcome::Exit)
            }
        },
        Commands::Routes { command, socket } => match command.unwrap_or(RoutesCommands::List { socket: socket.clone() }) {
            RoutesCommands::List { socket } => {
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
            RoutesCommands::Prune { socket } => {
                let mut client = IpcClient::connect(&socket).await?;
                let resp = client.prune_routes().await?;
                if let IpcData::PruneResult { pruned_count, pruned_domains, message } = resp {
                    println!("SUCCESS: {message}");
                    if !pruned_domains.is_empty() {
                        println!("Pruned domains ({}):", pruned_count);
                        for d in pruned_domains {
                            println!("  - {d}");
                        }
                    }
                } else {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                }
                Ok(CommandOutcome::Exit)
            }
        },
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
        Commands::Discovery {
            command,
            socket,
            docker_socket,
            local,
            json,
        } => {
            let cmd = command.unwrap_or(DiscoveryCommands::List {
                socket,
                docker_socket,
                local,
                json,
            });
            match cmd {
                DiscoveryCommands::List {
                    socket,
                    docker_socket,
                    local,
                    json,
                } => {
                    let mut services = None;
                    if !local {
                        if let Ok(mut client) = IpcClient::connect(&socket).await {
                            if let Ok(ipc_services) = client.list_discovery().await {
                                services = Some(ipc_services);
                            }
                        }
                    }

                    let services = match services {
                        Some(s) => s,
                        None => {
                            let sock_str = docker_socket.to_str().unwrap_or("/var/run/docker.sock");
                            crate::discovery::DockerDiscovery::scan_docker(sock_str, "local", None)
                                .await
                                .map_err(|e| format!("Failed to connect to daemon at '{}' and Docker at '{}': {e}", socket.display(), sock_str))?
                        }
                    };

                    print_discovered_services(&services, json);
                    Ok(CommandOutcome::Exit)
                }
                DiscoveryCommands::Scan {
                    docker_socket,
                    json,
                } => {
                    let sock_str = docker_socket.to_str().unwrap_or("/var/run/docker.sock");
                    let services = crate::discovery::DockerDiscovery::scan_docker(sock_str, "local", None)
                        .await
                        .map_err(|e| format!("Docker scan failed at '{sock_str}': {e}"))?;
                    print_discovered_services(&services, json);
                    Ok(CommandOutcome::Exit)
                }
            }
        }
        Commands::Update {
            check,
            force,
            version,
            repo,
        } => {
            let client = reqwest::Client::new();
            if check {
                match crate::updater::check_update(&client, &repo, version.as_deref()).await {
                    Ok(info) => {
                        println!("Current version: v{}", info.current_version);
                        println!("Target platform: {}", info.target_platform);
                        println!("Latest release:  {}", info.latest_tag);
                        if info.is_newer {
                            println!("\nUpdate available! Run 'bridge update' to install.");
                        } else {
                            println!("\nBridge is up to date.");
                        }
                        Ok(CommandOutcome::Exit)
                    }
                    Err(err) => {
                        eprintln!("Error checking for updates: {err}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::updater::perform_update(&client, &repo, version.as_deref(), force).await {
                    Ok(msg) => {
                        println!("{msg}");
                        Ok(CommandOutcome::Exit)
                    }
                    Err(err) => {
                        eprintln!("Error updating bridge: {err}");
                        std::process::exit(1);
                    }
                }
            }
        }
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

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m {s}s")
    } else if hours > 0 {
        format!("{hours}h {mins}m {s}s")
    } else if mins > 0 {
        format!("{mins}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn print_cluster_nodes(status: &IpcData) -> Result<(), Box<dyn std::error::Error>> {
    if let IpcData::Cluster {
        local_node_id,
        current_leader,
        is_leader,
        peers,
    } = status
    {
        let leader_str = current_leader.as_deref().unwrap_or("None");
        println!(
            "LOCAL NODE: {} ({}) | LEADER: {}",
            local_node_id,
            if *is_leader { "LEADER" } else { "FOLLOWER" },
            leader_str
        );
        println!(
            "\n{:<16} {:<18} {:<24} {:<12} {:<8}",
            "NODE ID", "MESH IP", "ENDPOINT", "ROLE", "PRIORITY"
        );
        println!("{:-<80}", "");
        for p in peers {
            let role = if p.is_leader { "LEADER" } else { "PEER" };
            let is_self = if &p.node_id == local_node_id { " (self)" } else { "" };
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
        println!("{}", serde_json::to_string_pretty(status)?);
    }
    Ok(())
}

pub fn print_discovered_services(services: &[crate::ipc::DiscoveredServiceInfo], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(services).unwrap_or_else(|_| "[]".to_string()));
        return;
    }

    if services.is_empty() {
        println!("No auto-discovered Docker services found.");
        println!();
        println!("Ensure containers are running with supported routing labels:");
        println!("  - Traefik:  traefik.http.routers.<name>.rule=Host(`example.com`)");
        println!("  - Coolify:  coolify.domain=example.com");
        println!("  - Bridge:   bridge.domain=example.com");
        println!("  - Caddy:    caddy_0=example.com");
        return;
    }

    println!("{:<14} {:<18} {:<20} {:<32} {:<18} {:<10}", "CONTAINER ID", "NAME", "IMAGE", "DOMAINS", "UPSTREAM", "STATUS");
    println!("{:-<116}", "");
    for s in services {
        let domains = s.domains.join(", ");
        let upstream = s.upstream.map(|a| a.to_string()).unwrap_or_else(|| format!("127.0.0.1:{}", s.port));
        let short_image = if s.image.len() > 19 {
            format!("{}...", &s.image[..16])
        } else {
            s.image.clone()
        };
        let short_name = if s.container_name.len() > 17 {
            format!("{}...", &s.container_name[..14])
        } else {
            s.container_name.clone()
        };
        let short_domains = if domains.len() > 31 {
            format!("{}...", &domains[..28])
        } else {
            domains
        };
        println!("{:<14} {:<18} {:<20} {:<32} {:<18} {:<10}", s.container_id, short_name, short_image, short_domains, upstream, s.status);
    }
}
