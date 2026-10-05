use std::net::SocketAddr;
use std::path::PathBuf;
use clap::{Parser, Subcommand};

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
}
