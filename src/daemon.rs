use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use proxy::ProxyConfig;
use registry::{Node, Route};

use crate::core::config::Config;
use crate::core::telemetry::init_telemetry;

/// Runs the Bridge daemon with the given configuration path.
pub async fn run(config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load_auto(config_path)?;

    // Unified logger and Sentry telemetry initialization
    let _telemetry_guard = init_telemetry(&config);

    // Map nodes by node_id for rapid target lookup
    let node_map: HashMap<&String, &Node> = config.nodes.iter().map(|n| (&n.node_id, n)).collect();

    // 1. Build initial domain routing table from static config
    let routes = build_initial_routes(&config, &node_map);

    // 2. Prepare and validate proxy and UDP service configurations
    let proxy_config = configure_proxy_and_udp(&config, &node_map);

    // 3. Build domain registry
    let registry = Arc::new(registry::DomainRegistry::with_routes(routes));
    let proxy = proxy::Proxy::new(Arc::new(proxy_config.clone()), registry.clone());

    let mut coordinator = proxy::ShutdownCoordinator::new(Some(std::path::PathBuf::from("bridge-state.json")));

    // 4. Register and spawn Docker Auto-Discovery if enabled
    let discovery_services_store = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    if config.discovery.enabled {
        let (discovery_rx, discovery_ack) = coordinator.register_subsystem("docker_discovery");
        let disc_reg = registry.clone();
        let default_node_id = config
            .node
            .as_ref()
            .map(|n| n.id.clone())
            .unwrap_or_else(|| config.discovery.default_node_id.clone());
        let discovery_mesh_ip = config.node.as_ref().map(|n| n.mesh_ip);
        let socket_path = config.discovery.docker_socket.clone();
        let store = discovery_services_store.clone();

        tokio::spawn(async move {
            match crate::discovery::DockerDiscovery::connect_socket(&socket_path, disc_reg, default_node_id) {
                Ok(discovery) => {
                    let discovery = discovery.with_services_store(store);
                    let discovery = match discovery_mesh_ip {
                        Some(mesh_ip) => discovery.with_mesh_ip(mesh_ip),
                        None => discovery,
                    };
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

    // 5. Register and spawn Unix Domain Socket IPC control server if enabled
    let auth_manager = Arc::new(crate::auth::AuthManager::new(&config.auth));
    let (ipc_cluster_slot, ipc_duplicator_slot) = if config.ipc.enabled {
        let (ipc_rx, ipc_ack) = coordinator.register_subsystem("ipc_server");
        #[allow(unused_mut)]
        let mut ipc_server = crate::ipc::IpcServer::new(
            &config.ipc.socket_path,
            registry.clone(),
            proxy_config.mode,
        );
        ipc_server.set_discovery_handle(discovery_services_store.clone());
        let cluster_slot = ipc_server.cluster_handle();
        let duplicator_slot = ipc_server.duplicator_handle();
        *ipc_server.auth_handle().write().await = Some(auth_manager.clone());

        // 6. Register and spawn HTTP Dashboard if enabled and compiled in (before IPC spawn so
        // both subsystems share the same configuration slots).
        #[cfg(feature = "dashboard")]
        if config.dashboard.enabled {
            let (dash_rx, dash_ack) = coordinator.register_subsystem("dashboard");
            let dash_server = Arc::new(crate::dashboard::DashboardServer::new(
                config.dashboard.listen_addr,
                registry.clone(),
                cluster_slot.clone(),
                duplicator_slot.clone(),
                auth_manager.clone(),
            ));
            dash_server.set_public_domain(config.dashboard.public_domain.clone());
            ipc_server.set_config_slots(
                dash_server.config_json.clone(),
                dash_server.config_path.clone(),
            );
            if let Some(config_view) = load_config_view(&config) {
                dash_server.set_config(config_view, config.loaded_from.clone());
            }
            tokio::spawn(async move {
                if let Err(err) = dash_server.run_with_shutdown(dash_rx, Some(dash_ack)).await {
                    tracing::warn!(%err, "dashboard server stopped with error");
                }
            });
        }

        #[cfg(feature = "dashboard")]
        let dashboard_running = config.dashboard.enabled;
        #[cfg(not(feature = "dashboard"))]
        let dashboard_running = false;

        if !dashboard_running {
            if let Some(config_view) = load_config_view(&config) {
                *ipc_server.config_view_handle().write().await = Some(config_view);
            }
            if let Some(path) = config.loaded_from.clone() {
                *ipc_server.config_path_handle().write().await = Some(path);
            }
        }

        tokio::spawn(async move {
            if let Err(err) = ipc_server.run_with_shutdown(ipc_rx, Some(ipc_ack)).await {
                tracing::warn!(%err, "IPC server stopped with error");
            }
        });
        (cluster_slot, duplicator_slot)
    } else {
        #[cfg(feature = "dashboard")]
        if config.dashboard.enabled {
            let (dash_rx, dash_ack) = coordinator.register_subsystem("dashboard");
            let dash_server = Arc::new(crate::dashboard::DashboardServer::new(
                config.dashboard.listen_addr,
                registry.clone(),
                Arc::new(tokio::sync::RwLock::new(None)),
                Arc::new(tokio::sync::RwLock::new(None)),
                auth_manager.clone(),
            ));
            dash_server.set_public_domain(config.dashboard.public_domain.clone());
            if let Some(config_view) = load_config_view(&config) {
                dash_server.set_config(config_view, config.loaded_from.clone());
            }
            tokio::spawn(async move {
                if let Err(err) = dash_server.run_with_shutdown(dash_rx, Some(dash_ack)).await {
                    tracing::warn!(%err, "dashboard server stopped with error");
                }
            });
        }
        (
            Arc::new(tokio::sync::RwLock::new(None)),
            Arc::new(tokio::sync::RwLock::new(None)),
        )
    };

    // 7. Register and spawn raw UDP Cluster Mesh if node configuration is present
    if let Some(node_cfg) = &config.node {
        let cluster_slot_clone = ipc_cluster_slot.clone();
        let duplicator_slot_clone = ipc_duplicator_slot.clone();
        let (cluster_rx, cluster_ack) = coordinator.register_subsystem("cluster_mesh");
        let (priv_key, pub_key) = match (&node_cfg.private_key, &node_cfg.public_key) {
            (Some(privk), Some(pubk)) => (privk.clone(), pubk.clone()),
            _ => crate::mesh::generate_wireguard_keypair(),
        };

        let peer_node = crate::cluster::PeerNode::new(
            &node_cfg.id,
            pub_key.clone(),
            node_cfg.mesh_ip,
            node_cfg.endpoint,
        )
        .with_priority(node_cfg.priority)
        .with_listen_port(node_cfg.listen_port);

        let wireguard = Arc::new(tokio::sync::RwLock::new(crate::mesh::WireGuardDevice::new(
            "wg0",
            priv_key,
            pub_key,
            node_cfg.listen_port,
            node_cfg.mesh_ip,
        )));

        let cluster_reg = registry.clone();
        let seeds: Vec<std::net::SocketAddr> = config.seeds.iter().map(|s| s.endpoint).collect();

        // Ingress Handoff setup (Tunnel or DNS)
        let tunnel_handoff_setup = if config.handoff.mode == crate::core::config::HandoffMode::Tunnel {
            if let Some(tunnel_cfg) = &config.handoff.tunnel {
                let (tunnel_rx, tunnel_ack) = coordinator.register_subsystem("tunnel_handoff");
                let mgr = Arc::new(crate::handoff::TunnelManager::new(
                    tunnel_cfg.clone(),
                    node_cfg.id.clone(),
                ));
                Some((mgr, tunnel_rx, tunnel_ack))
            } else {
                None
            }
        } else {
            None
        };

        let dns_handoff_setup = if config.handoff.mode == crate::core::config::HandoffMode::Dns {
            if let Some(dns_cfg) = &config.handoff.dns {
                let (dns_rx, dns_ack) = coordinator.register_subsystem("dns_handoff");
                let mgr = Arc::new(crate::handoff::DnsManager::new(
                    dns_cfg.clone(),
                    node_cfg.id.clone(),
                    node_cfg.endpoint.ip(),
                ));
                Some((mgr, dns_rx, dns_ack))
            } else {
                None
            }
        } else {
            None
        };

        let failover_setup = if config.services.iter().any(|s| s.replicate.as_ref().is_some_and(|r| r.enabled)) {
            let (failover_rx, failover_ack) = coordinator.register_subsystem("service_failover");
            Some((failover_rx, failover_ack))
        } else {
            None
        };
        let failover_services = config.services.clone();
        let docker_socket = config.discovery.docker_socket.clone();
        let local_node_id = node_cfg.id.clone();
        let dashboard_public_domain = config.dashboard.public_domain.clone();
        let dashboard_listen_addr = config.dashboard.listen_addr;

        tokio::spawn(async move {
            match crate::cluster::ClusterController::bind(peer_node, wireguard, cluster_reg.clone()).await {
                Ok(ctrl) => {
                    let ctrl = Arc::new(ctrl);
                    *cluster_slot_clone.write().await = Some(ctrl.clone());

                    // Leader-only public dashboard ingress: the dashboard route
                    // exists only on the elected leader and never gossips.
                    if let Some(public_domain) = dashboard_public_domain.clone() {
                        ctrl.set_local_only_domains(HashSet::from([public_domain.clone()]))
                            .await;
                        let dash_reg = cluster_reg.clone();
                        let local_id = local_node_id.clone();
                        let dash_addr = dashboard_listen_addr;
                        let mut leader_rx = ctrl.leader_watch();
                        tokio::spawn(async move {
                            loop {
                                let leader = leader_rx.borrow().clone();
                                apply_dashboard_ownership(
                                    &dash_reg,
                                    &public_domain,
                                    &local_id,
                                    leader.as_ref().map(|l| l.node_id.as_str()),
                                    dash_addr,
                                );
                                if leader_rx.changed().await.is_err() {
                                    break;
                                }
                            }
                        });
                    }

                    // If tunnel handoff is enabled, spawn the reactive watcher
                    if let Some((mgr, tunnel_rx, tunnel_ack)) = tunnel_handoff_setup {
                        let leader_rx = ctrl.leader_watch();
                        tokio::spawn(async move {
                            if let Err(err) = mgr.run_with_shutdown(leader_rx, tunnel_rx, Some(tunnel_ack)).await {
                                tracing::warn!(%err, "tunnel handoff manager error");
                            }
                        });
                    }

                    // If DNS handoff is enabled, spawn the reactive watcher
                    if let Some((mgr, dns_rx, dns_ack)) = dns_handoff_setup {
                        let leader_rx = ctrl.leader_watch();
                        tokio::spawn(async move {
                            if let Err(err) = mgr.run_with_shutdown(leader_rx, dns_rx, Some(dns_ack)).await {
                                tracing::warn!(%err, "dns handoff manager error");
                            }
                        });
                    }

                    // Initialize service failover duplicator
                    let duplicator = Arc::new(crate::failover::WorkloadDuplicator::new(
                        local_node_id,
                        failover_services,
                        cluster_reg,
                        Arc::new(crate::failover::DockerContainerDriver::new(docker_socket)),
                        ctrl.clone(),
                    ));
                    *duplicator_slot_clone.write().await = Some(duplicator.clone());

                    // If service failover duplication is enabled, spawn the reactive event loop
                    if let Some((failover_rx, failover_ack)) = failover_setup {
                        let member_rx = ctrl.subscribe_membership();
                        let command_rx = ctrl.subscribe_workload_commands();
                        tokio::spawn(async move {
                            if let Err(err) = duplicator.run_with_shutdown(member_rx, command_rx, failover_rx, Some(failover_ack)).await {
                                tracing::warn!(%err, "service failover duplicator error");
                            }
                        });
                    }

                    if let Err(err) = ctrl.bootstrap(&seeds).await {
                        tracing::warn!(%err, "cluster bootstrap warning");
                    }
                    if let Err(err) = ctrl.run_with_shutdown(cluster_rx, Some(cluster_ack)).await {
                        tracing::warn!(%err, "cluster controller stopped with error");
                    }
                }
                Err(err) => {
                    tracing::warn!(%err, "failed to bind cluster UDP socket");
                    let _ = cluster_ack
                        .send(proxy::ShutdownAck {
                            subsystem: "cluster_mesh".to_string(),
                            details: Some(err.to_string()),
                        })
                        .await;
                    if let Some((_, failover_ack)) = failover_setup {
                        let _ = failover_ack
                            .send(proxy::ShutdownAck {
                                subsystem: "service_failover".to_string(),
                                details: Some(err.to_string()),
                            })
                            .await;
                    }
                }
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
        dashboard = config.dashboard.enabled,
        "bridge is running"
    );

    proxy.run_with_coordinator(&mut coordinator).await?;

    Ok(())
}

/// Derives dashboard route ownership directly from leader-election state.
///
/// The public dashboard route exists only on the current leader; every
/// leadership change moves it without any separate persistence.
pub fn apply_dashboard_ownership(
    registry: &registry::DomainRegistry,
    public_domain: &str,
    local_node_id: &str,
    leader_id: Option<&str>,
    dashboard_addr: std::net::SocketAddr,
) {
    if leader_id == Some(local_node_id) {
        let node = Node::new(local_node_id.to_string(), dashboard_addr);
        let route = Route::new(Some(dashboard_addr), node);
        registry.insert_versioned(public_domain.to_string(), route);
        tracing::info!(domain = %public_domain, "public dashboard route activated on leader");
    } else {
        registry.remove(public_domain);
    }
}

/// Builds a JSON view of the raw configuration file for the dashboard's
/// read-only config display. `Config` is not `Serialize`, so the source
/// file it was loaded from is converted directly (TOML or YAML).
pub fn load_config_view(config: &Config) -> Option<serde_json::Value> {
    let path = config.loaded_from.as_ref()?;
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => {
            tracing::warn!(%e, path = %path.display(), "dashboard: config view read failed");
            return None;
        }
    };
    let is_yaml = matches!(path.extension().and_then(|e| e.to_str()), Some("yaml" | "yml"));
    let view = if is_yaml {
        serde_yaml::from_str::<serde_json::Value>(&raw).ok()
    } else {
        toml::from_str::<serde_json::Value>(&raw).ok()
    };
    if view.is_none() {
        tracing::warn!(path = %path.display(), "dashboard: config view conversion failed");
    }
    view
}

pub fn build_initial_routes(config: &Config, node_map: &HashMap<&String, &Node>) -> HashMap<String, Route> {
    let mut routes: HashMap<String, Route> = HashMap::new();
    for service in &config.services {
        let Some(host) = service.url.host_str() else {
            tracing::warn!(url = %service.url, "service URL has no host");
            continue;
        };
        if let Some(node) = node_map.get(&service.node_id) {
            routes
                .entry(host.to_string())
                .and_modify(|r| r.add_target((*node).clone()))
                .or_insert_with(|| Route::new(service.upstream, (*node).clone()));
        } else {
            tracing::warn!(
                url = %service.url,
                node_id = %service.node_id,
                "node not found in nodes table"
            );
        }
    }
    routes
}

fn configure_proxy_and_udp(config: &Config, node_map: &HashMap<&String, &Node>) -> ProxyConfig {
    let mut proxy_config = config.proxy.clone();
    proxy_config.udp_services.extend(config.udp_services.clone());
    if proxy_config.managed.dashboard_domain.is_none() {
        proxy_config.managed.dashboard_domain = config.dashboard.public_domain.clone();
    }

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

    proxy_config
}
