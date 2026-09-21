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

    // Build domain registry and populate it from static config if present
    let registry = Arc::new(registry::DomainRegistry::with_routes(routes.clone()));

    let proxy = proxy::Proxy::new(Arc::new(config.proxy.clone()), registry.clone());

    tracing::info!(
        mode = ?config.proxy.mode,
        routes = registry.len(),
        telemetry = config.enable_telemetry,
        "bridge is running"
    );

    proxy.run().await?;

    Ok(())
}
