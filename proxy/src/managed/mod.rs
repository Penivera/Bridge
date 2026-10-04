use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use registry::{DomainRegistry, RoutingPreference};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::core::config::ManagedConfig;
use crate::shutdown::ShutdownReason;

/// Root dynamic configuration structure for Traefik v2/v3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraefikConfig {
    pub http: TraefikHttp,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraefikHttp {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routers: BTreeMap<String, TraefikRouter>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, TraefikService>,
}

impl TraefikHttp {
    pub fn is_empty(&self) -> bool {
        self.routers.is_empty() && self.services.is_empty()
    }
}

/// Traefik HTTP router configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraefikRouter {
    pub rule: String,
    pub service: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TraefikTls>,
}

/// Traefik TLS router configuration block.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraefikTls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_resolver: Option<String>,
}

/// Traefik HTTP service definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraefikService {
    pub load_balancer: TraefikLoadBalancer,
}

/// Traefik load balancer definition containing server endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraefikLoadBalancer {
    pub servers: Vec<TraefikServer>,
}

/// A single upstream target server URL in Traefik.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraefikServer {
    pub url: String,
}

/// Sanitizes a domain name into a DNS/identifier-safe Traefik router and service key.
pub fn sanitize_traefik_name(domain: &str) -> String {
    let sanitized: String = domain
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "bridge-default".to_string()
    } else {
        format!("bridge-{trimmed}")
    }
}

/// Formats the Traefik rule for matching the incoming Host header.
pub fn traefik_host_rule(domain: &str) -> String {
    let host = domain.split(':').next().unwrap_or(domain);
    format!("Host(`{host}`)")
}

/// Determines whether a route targets the local node (and should therefore be skipped).
pub fn is_local_route(node_id: &str, local_node_id: &str) -> bool {
    node_id == "self" || node_id == local_node_id
}

/// Translates the DomainRegistry entries into a Traefik dynamic configuration struct.
/// Routes targeting the local node are skipped so Coolify's local Docker discovery is untouched.
pub fn generate_traefik_config(
    registry: &DomainRegistry,
    routing_pref: RoutingPreference,
    config: &ManagedConfig,
) -> TraefikConfig {
    let snapshot = registry.snapshot();
    let mut routers = BTreeMap::new();
    let mut services = BTreeMap::new();

    // Sort by domain for deterministic output
    let mut sorted_entries: Vec<(&String, &registry::Route)> = snapshot.iter().collect();
    sorted_entries.sort_by_key(|(domain, _)| (*domain).clone());

    for (domain, route) in sorted_entries {
        if is_local_route(&route.node.node_id, &config.local_node_id) {
            continue;
        }

        let base_name = sanitize_traefik_name(domain);
        let mut name = base_name.clone();
        let mut counter = 1;
        while routers.contains_key(&name) {
            counter += 1;
            name = format!("{base_name}-{counter}");
        }

        let target_addr = route.target_addr_with_fallback(routing_pref);
        let scheme = if target_addr.port() == 443 {
            "https"
        } else {
            &config.default_target_scheme
        };
        let server_url = format!("{scheme}://{target_addr}");

        let tls = if config.tls_enabled {
            Some(TraefikTls {
                cert_resolver: config.cert_resolver.clone(),
            })
        } else {
            None
        };

        let router = TraefikRouter {
            rule: traefik_host_rule(domain),
            service: name.clone(),
            entry_points: config.entrypoints.clone(),
            tls,
        };

        let service = TraefikService {
            load_balancer: TraefikLoadBalancer {
                servers: vec![TraefikServer { url: server_url }],
            },
        };

        routers.insert(name.clone(), router);
        services.insert(name, service);
    }

    TraefikConfig {
        http: TraefikHttp { routers, services },
    }
}

/// Serializes a `TraefikConfig` into YAML with an informative header.
pub fn traefik_config_to_yaml(config: &TraefikConfig) -> Result<String, serde_yaml::Error> {
    let yaml = serde_yaml::to_string(config)?;
    Ok(format!(
        "# Auto-generated by Bridge daemon in Managed mode. Do not edit manually.\n{yaml}"
    ))
}

/// Writes Traefik dynamic configuration content to the specified path atomically.
/// Returns `Ok(true)` if the file was written, or `Ok(false)` if disk content was already identical.
pub fn sync_traefik_file(path: &Path, content: &str) -> std::io::Result<bool> {
    if path.exists()
        && let Ok(existing) = std::fs::read_to_string(path)
        && existing == content
    {
        return Ok(false);
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = if let Some(parent) = path.parent() {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bridge.yaml");
        parent.join(format!(".{file_name}.{}.tmp", std::process::id()))
    } else {
        PathBuf::from(format!(".bridge.{}.tmp", std::process::id()))
    };

    if std::fs::write(&tmp_path, content).is_ok() {
        if std::fs::rename(&tmp_path, path).is_ok() {
            return Ok(true);
        }
        let _ = std::fs::remove_file(&tmp_path);
    }

    std::fs::write(path, content)?;
    Ok(true)
}

/// Generates the Traefik dynamic configuration from the current `DomainRegistry`
/// and flushes it to the configured `dynamic_config_path`.
pub fn sync_dynamic_config(
    config: &ManagedConfig,
    registry: &DomainRegistry,
    routing_pref: RoutingPreference,
) -> std::io::Result<bool> {
    let traefik_config = generate_traefik_config(registry, routing_pref, config);
    let yaml = traefik_config_to_yaml(&traefik_config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    sync_traefik_file(&config.dynamic_config_path, &yaml)
}

/// Runs the Managed mode Traefik dynamic configuration provider loop, synchronizing
/// configuration at regular intervals and upon graceful shutdown.
pub async fn run_managed_provider_with_shutdown(
    config: ManagedConfig,
    registry: Arc<DomainRegistry>,
    routing_pref: RoutingPreference,
    mut shutdown_rx: broadcast::Receiver<ShutdownReason>,
    ack_tx: mpsc::Sender<crate::shutdown::ShutdownAck>,
) -> std::io::Result<()> {
    tracing::info!(
        path = %config.dynamic_config_path.display(),
        "managed traefik provider started"
    );

    // Initial configuration sync
    if let Err(e) = sync_dynamic_config(&config, &registry, routing_pref) {
        tracing::error!(
            path = %config.dynamic_config_path.display(),
            error = %e,
            "initial traefik dynamic configuration sync failed"
        );
    }

    let mut interval = tokio::time::interval(config.sync_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                match sync_dynamic_config(&config, &registry, routing_pref) {
                    Ok(true) => {
                        tracing::debug!(
                            path = %config.dynamic_config_path.display(),
                            "traefik dynamic config updated"
                        );
                    }
                    Ok(false) => {}
                    Err(e) => {
                        tracing::warn!(
                            path = %config.dynamic_config_path.display(),
                            error = %e,
                            "periodic traefik dynamic config sync failed"
                        );
                    }
                }
            }
            reason = shutdown_rx.recv() => {
                tracing::info!(reason = ?reason, "managed traefik provider shutting down");
                let updated = sync_dynamic_config(&config, &registry, routing_pref).unwrap_or(false);
                let _ = ack_tx.send(crate::shutdown::ShutdownAck {
                    subsystem: "managed_traefik_provider".to_string(),
                    details: Some(format!("final_sync_updated={}, path={}", updated, config.dynamic_config_path.display())),
                }).await;
                break;
            }
        }
    }

    Ok(())
}

