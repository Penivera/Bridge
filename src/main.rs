use bridge::core::config::Config;
use bridge::core::telemetry::init_telemetry;
use std::sync::Arc;
mod cli;
use clap::Parser;
use cli::Args;
use registry::{Node, Route};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();

    // Handle CLI subcommands (querying running daemon via UDS)
    if let Some(command) = args.command {
        match command {
            cli::Commands::Ping { socket } => {
                let mut client = bridge::ipc::IpcClient::connect(&socket).await?;
                let resp = client.ping().await?;
                println!("Bridge IPC ping: {resp}");
                return Ok(());
            }
            cli::Commands::Status { socket } => {
                let mut client = bridge::ipc::IpcClient::connect(&socket).await?;
                let status = client.status().await?;
                println!("{}", serde_json::to_string_pretty(&status)?);
                return Ok(());
            }
            cli::Commands::Routes { socket } => {
                let mut client = bridge::ipc::IpcClient::connect(&socket).await?;
                let routes = client.list_routes().await?;
                if routes.is_empty() {
                    println!("No routes registered.");
                } else {
                    println!("{:<32} {:<24} {:<15}", "DOMAIN", "UPSTREAM", "NODE_ID");
                    println!("{:-<72}", "");
                    for (domain, info) in routes {
                        let up = info
                            .upstream
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "-".to_string());
                        println!("{:<32} {:<24} {:<15}", domain, up, info.node_id);
                    }
                }
                return Ok(());
            }
            cli::Commands::AddRoute {
                domain,
                upstream,
                node_id,
                socket,
            } => {
                let mut client = bridge::ipc::IpcClient::connect(&socket).await?;
                client.add_route(&domain, upstream, node_id).await?;
                println!("Successfully added route for domain: {domain}");
                return Ok(());
            }
            cli::Commands::RemoveRoute { domain, socket } => {
                let mut client = bridge::ipc::IpcClient::connect(&socket).await?;
                let removed = client.remove_route(&domain).await?;
                if removed {
                    println!("Successfully removed route for domain: {domain}");
                } else {
                    println!("Route for domain '{domain}' did not exist.");
                }
                return Ok(());
            }
            cli::Commands::Run { config: run_cfg } => {
                if run_cfg.is_some() {
                    args.config = run_cfg;
                }
            }
        }
    }

    let config = Config::load_auto(args.config.as_deref())?;

    // Single unified call for logger and Sentry telemetry initialization
    let _telemetry_guard = init_telemetry(&config);

    let mut routes: HashMap<String, Route> = HashMap::new();
    // Map nodes by node_id for lookup
    let node_map: HashMap<&String, &Node> = config.nodes.iter().map(|n| (&n.node_id, n)).collect();

    for service in &config.services {
        let Some(host) = service.url.host_str() else {
            tracing::warn!(url = %service.url, "service URL has no host");
            continue;
        };
        if let Some(node) = node_map.get(&service.node_id) {
            routes.insert(
                host.to_string(),
                Route::new(service.upstream, (*node).clone()),
            );
        } else {
            tracing::warn!(
                url = %service.url,
                node_id = %service.node_id,
                "node not found in nodes table"
            );
        }
    }

    let mut proxy_config = config.proxy.clone();
    proxy_config.udp_services.extend(config.udp_services.clone());

    for udp_service in &mut proxy_config.udp_services {
        if udp_service.node_id != "self" {
            if let Some(node) = node_map.get(&udp_service.node_id) {
                if udp_service.upstream.ip().is_loopback() || udp_service.upstream.ip().is_unspecified() {
                    udp_service.upstream = std::net::SocketAddr::new(
                        node.target_address_with_fallback(proxy_config.routing).ip(),
                        udp_service.listen_port,
                    );
                }
            } else {
                tracing::warn!(
                    node_id = %udp_service.node_id,
                    listen_port = udp_service.listen_port,
                    "node not found in nodes table for udp service"
                );
            }
        } else if udp_service.upstream.port() == udp_service.listen_port
            && (udp_service.listen_addr.ip().is_unspecified() || udp_service.listen_addr.ip().is_loopback())
            && udp_service.upstream.ip().is_loopback()
        {
            tracing::warn!(
                listen_port = udp_service.listen_port,
                upstream = %udp_service.upstream,
                "udp service for 'self' has upstream pointing to its own listen port; an explicit upstream port should be specified"
            );
        }
    }

    // Build domain registry and populate it from static config if present
    let registry = Arc::new(registry::DomainRegistry::with_routes(routes.clone()));

    let proxy = proxy::Proxy::new(Arc::new(proxy_config.clone()), registry.clone());

    let mut coordinator = proxy::ShutdownCoordinator::new(Some(std::path::PathBuf::from("bridge-state.json")));

    // 1. Register and spawn Docker Auto-Discovery if enabled
    if config.discovery.enabled {
        let (discovery_rx, discovery_ack) = coordinator.register_subsystem("docker_discovery");
        let disc_reg = registry.clone();
        let default_node_id = config.discovery.default_node_id.clone();
        let socket_path = config.discovery.docker_socket.clone();

        tokio::spawn(async move {
            match bridge::discovery::DockerDiscovery::connect_socket(&socket_path, disc_reg, default_node_id) {
                Ok(discovery) => {
                    if let Err(err) = discovery.run_with_shutdown(discovery_rx, Some(discovery_ack)).await {
                        tracing::warn!(%err, "docker discovery service stopped with error");
                    }
                }
                Err(err) => {
                    tracing::warn!(%err, socket = %socket_path, "docker daemon unavailable for auto-discovery; continuing without docker discovery");
                    let _ = discovery_ack
                        .send(proxy::ShutdownAck {
                            subsystem: "docker_discovery".to_string(),
                            details: Some(err.to_string()),
                        })
                        .await;
                }
            }
        });
    }

    // 2. Register and spawn Unix Domain Socket IPC control server if enabled
    if config.ipc.enabled {
        let (ipc_rx, ipc_ack) = coordinator.register_subsystem("ipc_server");
        let ipc_server = bridge::ipc::IpcServer::new(
            &config.ipc.socket_path,
            registry.clone(),
            proxy_config.mode,
        );
        tokio::spawn(async move {
            if let Err(err) = ipc_server.run_with_shutdown(ipc_rx, Some(ipc_ack)).await {
                tracing::warn!(%err, "IPC server stopped with error");
            }
        });
    }

    tracing::info!(
        mode = ?proxy.config.mode,
        routes = registry.len(),
        udp_services = proxy.config.udp_services.len(),
        telemetry = config.enable_telemetry,
        docker_discovery = config.discovery.enabled,
        ipc = config.ipc.enabled,
        "bridge is running"
    );

    proxy.run_with_coordinator(&mut coordinator).await?;

    Ok(())
}
