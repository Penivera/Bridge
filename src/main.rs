use std::sync::Arc;
use bridge::core::config::Config;
use bridge::core::telemetry::init_telemetry;
mod cli;
use clap::Parser;
use cli::Args;
use registry::Node;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = Config::load_auto(args.config.as_deref())?;

    // Single unified call for logger and Sentry telemetry initialization
    let _telemetry_guard = init_telemetry(&config);

    // Build domain registry and populate it from static config if present
    let registry = Arc::new(registry::DomainRegistry::new());

    // Map nodes by node_id for lookup
    let node_map: std::collections::HashMap<&String, &Node> = config
        .proxy
        .nodes
        .iter()
        .map(|n| (&n.node_id, n))
        .collect();

    for service in &config.services {
        if let Some(node) = node_map.get(&service.node_id) {
            registry.insert(service.url.clone(), (*node).clone());
        } else {
            tracing::warn!(
                url = %service.url,
                node_id = %service.node_id,
                "node not found in proxy.nodes table"
            );
        }
    }

    tracing::info!(
        mode = ?config.proxy.mode,
        routes = registry.len(),
        telemetry = config.enable_telemetry,
        "bridge is running"
    );

    Ok(())
}
