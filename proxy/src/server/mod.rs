use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use registry::DomainRegistry;
use tokio::net::TcpListener;
use hyper_util::{rt::TokioExecutor,client::legacy::{Client,connect::HttpConnector}};
use crate::core::config::ProxyConfig;
use crate::core::enums::ProxyMode;

mod handler;

/// Core ingress proxy engine for Bridge.
#[derive(Clone)]
pub struct Proxy {
    pub config: Arc<ProxyConfig>,
    pub registry: Arc<DomainRegistry>,
    pub client: Client<HttpConnector,BoxBody<Bytes,hyper::Error>>
}

impl Proxy {
    /// Creates a new `Proxy` instance with the given configuration and domain registry.
    pub fn new(config: Arc<ProxyConfig>, registry: Arc<DomainRegistry>) -> Self {
        let mut  connector = HttpConnector::new();
        connector.set_keepalive_interval(Some(config.keep_alive_duration));
        connector.enforce_http(false);
        connector.set_tcp_user_timeout(Some(config.tcp_user_timeout));
        connector.set_happy_eyeballs_timeout(config.happy_eyeballs_timeout);
        connector.set_keepalive_retries(config.keep_alive_retries);
        let client = Client::builder(TokioExecutor::new())
            .build(connector);
        Self {
            config,
            registry,
            client
            
        }
    }

    /// Returns a reference to the proxy configuration.
    pub fn config(&self) -> &Arc<ProxyConfig> {
        &self.config
    }

    /// Returns a reference to the domain registry.
    pub fn registry(&self) -> &Arc<DomainRegistry> {
        &self.registry
    }

    /// Starts the proxy engine according to the configured `ProxyMode`, listening for
    /// OS termination signals (SIGINT and SIGTERM) to coordinate graceful shutdown.
    pub async fn run(&self) -> std::io::Result<()> {
        let default_state_path = std::path::PathBuf::from("bridge-state.json");
        let mut coordinator = crate::shutdown::ShutdownCoordinator::new(Some(default_state_path));
        self.run_with_coordinator(&mut coordinator).await
    }

    /// Runs the proxy engine with an existing `ShutdownCoordinator`, enabling broadcast
    /// teardown signaling and state finalization across all active subsystems.
    pub async fn run_with_coordinator(
        &self,
        coordinator: &mut crate::shutdown::ShutdownCoordinator,
    ) -> std::io::Result<()> {
        let has_udp = !self.config.udp_services.is_empty();
        let is_managed = self.config.mode == ProxyMode::Managed;
        let listeners = match self.config.mode {
            ProxyMode::Direct | ProxyMode::Handoff => {
                crate::transport::bind_listeners(&self.config).await?
            }
            ProxyMode::Managed => Vec::new(),
        };
        let has_tcp = !listeners.is_empty();

        if !has_tcp && !has_udp && !is_managed {
            return Ok(());
        }

        // Spawn state finalizer receiver if state file path is configured
        if let Some(path) = coordinator.state_file_path().cloned() {
            let (state_rx, state_ack) = coordinator.register_subsystem("state_finalizer");
            crate::shutdown::spawn_state_finalizer(
                path,
                format!("{:?}", self.config.mode),
                self.registry.clone(),
                self.config.udp_services.clone(),
                state_rx,
                state_ack,
            );
        }

        let mut subsystem_tasks = tokio::task::JoinSet::new();

        if is_managed {
            let (managed_rx, managed_ack) = coordinator.register_subsystem("managed_traefik_provider");
            let managed_config = self.config.managed.clone();
            let registry = self.registry.clone();
            let routing_pref = self.config.routing;
            subsystem_tasks.spawn(async move {
                crate::managed::run_managed_provider_with_shutdown(
                    managed_config,
                    registry,
                    routing_pref,
                    managed_rx,
                    managed_ack,
                ).await
            });
        }

        if has_tcp {
            let (tcp_rx, tcp_ack) = coordinator.register_subsystem("tcp_listeners");
            let svc = self.clone();
            subsystem_tasks.spawn(async move {
                crate::transport::run_listeners_with_shutdown(
                    listeners,
                    svc,
                    tcp_rx,
                    tcp_ack,
                ).await
            });
        }

        if has_udp {
            let (udp_rx, udp_ack) = coordinator.register_subsystem("udp_services");
            let services = self.config.udp_services.clone();
            subsystem_tasks.spawn(async move {
                crate::udp::run_udp_services_with_shutdown(
                    services,
                    udp_rx,
                    udp_ack,
                ).await
            });
        }

        // Spawn signal listener for SIGINT and SIGTERM
        let signal_fut = crate::shutdown::ShutdownCoordinator::wait_for_signal();

        tokio::select! {
            reason = signal_fut => {
                tracing::info!(%reason, "shutdown signal received; commencing graceful teardown");
                coordinator.broadcast_and_wait(reason, std::time::Duration::from_secs(10)).await;
                subsystem_tasks.shutdown().await;
            }
            res = subsystem_tasks.join_next() => {
                if let Some(result) = res {
                    match result {
                        Ok(Err(e)) => {
                            tracing::error!("subsystem error in proxy runtime: {e}");
                            return Err(e);
                        }
                        Err(e) => {
                            tracing::error!("subsystem task panicked: {e}");
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    /// Runs all configured UDP proxy services.
    pub async fn run_udp(&self) -> std::io::Result<()> {
        crate::udp::run_udp_services(self.config.udp_services.clone()).await
    }

    /// Runs the accept loop on the provided listeners to serve incoming traffic.
    pub async fn run_listeners(&self, listeners: Vec<TcpListener>) -> std::io::Result<()> {
        crate::transport::run_listeners(listeners, self.clone()).await
    }

    /// Manually triggers a dynamic configuration synchronization for Traefik in Managed mode.
    /// Returns `Ok(true)` if the dynamic configuration file was modified, or `Ok(false)` if it was already up to date.
    pub fn sync_managed_provider(&self) -> std::io::Result<bool> {
        crate::managed::sync_dynamic_config(&self.config.managed, &self.registry, self.config.routing)
    }
}

