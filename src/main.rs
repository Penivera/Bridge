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
    let args = Args::parse();
    let config = Config::load_auto(args.config.as_deref())?;

    // Single unified call for logger and Sentry telemetry initialization
    let _telemetry_guard = init_telemetry(&config);

    let mut routes:HashMap<String, Route> = HashMap::new();
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

    let proxy = proxy::Proxy::new(Arc::new(proxy_config), registry.clone());

    tracing::info!(
        mode = ?proxy.config.mode,
        routes = registry.len(),
        udp_services = proxy.config.udp_services.len(),
        telemetry = config.enable_telemetry,
        "bridge is running"
    );

    proxy.run().await?;

    Ok(())
}
