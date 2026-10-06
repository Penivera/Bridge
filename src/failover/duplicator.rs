use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cluster::{ClusterController, MemberEvent, PeerNode, WorkloadCommand};
use proxy::{ShutdownAck, ShutdownReason};
use registry::{DomainRegistry, HashRing, Node, Route};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{error, info, warn};

use crate::core::config::{FailbackMode, Service};

/// Trait abstracting container lifecycle operations (e.g. Docker, Mock, Podman).
pub trait ContainerDriver: Send + Sync + 'static {
    fn spawn_container(
        &self,
        domain: &str,
        image: &str,
        env: &[String],
        port: u16,
    ) -> impl std::future::Future<Output = Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>>> + Send;

    fn stop_container(
        &self,
        domain: &str,
    ) -> impl std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send;
}

/// In-memory mock container driver for deterministic testing without a live Docker daemon.
#[derive(Debug, Default, Clone)]
pub struct MockContainerDriver {
    pub spawned: Arc<Mutex<HashMap<String, SocketAddr>>>,
    pub stopped: Arc<Mutex<Vec<String>>>,
    pub base_port: Arc<std::sync::atomic::AtomicU16>,
}

impl MockContainerDriver {
    pub fn new() -> Self {
        Self {
            spawned: Arc::new(Mutex::new(HashMap::new())),
            stopped: Arc::new(Mutex::new(Vec::new())),
            base_port: Arc::new(std::sync::atomic::AtomicU16::new(18000)),
        }
    }

    pub async fn is_running(&self, domain: &str) -> bool {
        self.spawned.lock().await.contains_key(domain)
    }

    pub async fn spawned_addr(&self, domain: &str) -> Option<SocketAddr> {
        self.spawned.lock().await.get(domain).copied()
    }
}

impl ContainerDriver for MockContainerDriver {
    async fn spawn_container(
        &self,
        domain: &str,
        _image: &str,
        _env: &[String],
        _port: u16,
    ) -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let port = self
            .base_port
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        self.spawned.lock().await.insert(domain.to_string(), addr);
        info!(domain, %addr, "mock driver spawned container");
        Ok(addr)
    }

    async fn stop_container(
        &self,
        domain: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.spawned.lock().await.remove(domain);
        self.stopped.lock().await.push(domain.to_string());
        info!(domain, "mock driver stopped container");
        Ok(())
    }
}

/// Production Docker container driver interacting with the local Docker daemon via Bollard.
#[derive(Clone)]
pub struct DockerContainerDriver {
    socket_path: String,
}

impl DockerContainerDriver {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }
}

impl Default for DockerContainerDriver {
    fn default() -> Self {
        Self::new("/var/run/docker.sock")
    }
}

impl ContainerDriver for DockerContainerDriver {
    async fn spawn_container(
        &self,
        domain: &str,
        image: &str,
        env: &[String],
        port: u16,
    ) -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let docker = bollard::Docker::connect_with_unix(
            &self.socket_path,
            120,
            bollard::API_DEFAULT_VERSION,
        )?;

        let container_name = format!("bridge-replica-{}", domain.replace('.', "-"));

        let create_opts = bollard::query_parameters::CreateContainerOptionsBuilder::default()
            .name(&container_name)
            .build();

        let config = bollard::models::ContainerCreateBody {
            image: Some(image.to_string()),
            env: Some(env.to_vec()),
            ..Default::default()
        };

        docker.create_container(Some(create_opts), config).await?;
        docker
            .start_container(
                &container_name,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await?;

        let bound_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        info!(domain, container = %container_name, %bound_addr, "spawned replica container via Docker API");
        Ok(bound_addr)
    }

    async fn stop_container(
        &self,
        domain: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let docker = bollard::Docker::connect_with_unix(
            &self.socket_path,
            120,
            bollard::API_DEFAULT_VERSION,
        )?;
        let container_name = format!("bridge-replica-{}", domain.replace('.', "-"));

        let _ = docker
            .stop_container(
                &container_name,
                None::<bollard::query_parameters::StopContainerOptions>,
            )
            .await;

        let rm_opts = bollard::query_parameters::RemoveContainerOptionsBuilder::default()
            .force(true)
            .build();
        docker.remove_container(&container_name, Some(rm_opts)).await?;
        info!(domain, container = %container_name, "stopped and removed replica container via Docker API");
        Ok(())
    }
}

/// Metadata tracking a currently active duplicated workload.
#[derive(Debug, Clone)]
pub struct ActiveDuplication {
    pub domain: String,
    pub origin_node_id: String,
    pub assigned_node_id: String,
    pub failback_mode: FailbackMode,
    pub failback_cooldown_secs: u64,
    pub spawned_addr: SocketAddr,
    pub original_target_addr: Option<SocketAddr>,
}

/// Leader-coordinated service failover duplicator.
///
/// Discovers failed nodes via SWIM, selects replacement nodes via consistent hashing,
/// dispatches workload spawn commands, and handles configurable failback recovery.
pub struct WorkloadDuplicator<D: ContainerDriver> {
    pub local_node_id: String,
    pub services: Vec<Service>,
    pub registry: Arc<DomainRegistry>,
    pub driver: Arc<D>,
    pub cluster: Arc<ClusterController>,
    pub active_duplications: Arc<Mutex<HashMap<String, ActiveDuplication>>>,
}

impl<D: ContainerDriver> Clone for WorkloadDuplicator<D> {
    fn clone(&self) -> Self {
        Self {
            local_node_id: self.local_node_id.clone(),
            services: self.services.clone(),
            registry: self.registry.clone(),
            driver: self.driver.clone(),
            cluster: self.cluster.clone(),
            active_duplications: self.active_duplications.clone(),
        }
    }
}

impl<D: ContainerDriver> WorkloadDuplicator<D> {
    /// Creates a new `WorkloadDuplicator` orchestrator.
    pub fn new(
        local_node_id: impl Into<String>,
        services: Vec<Service>,
        registry: Arc<DomainRegistry>,
        driver: Arc<D>,
        cluster: Arc<ClusterController>,
    ) -> Self {
        Self {
            local_node_id: local_node_id.into(),
            services,
            registry,
            driver,
            cluster,
            active_duplications: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a snapshot of all currently active duplicated workloads.
    pub async fn active_duplications(&self) -> Vec<ActiveDuplication> {
        self.active_duplications.lock().await.values().cloned().collect()
    }

    /// Checks if this node is currently the elected cluster coordinator / leader.
    pub async fn is_leader(&self) -> bool {
        let election_state = self.cluster.election_state.read().await;
        election_state
            .current_leader
            .as_ref()
            .map(|l| l.node_id == self.local_node_id)
            .unwrap_or(false)
    }

    /// Handles a node failure event detected by SWIM.
    /// Only the elected leader initiates failover duplication to avoid thundering-herd spawns.
    pub async fn handle_member_down(
        &self,
        dead_node: &PeerNode,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        if !self.is_leader().await {
            return Ok(0);
        }
        self.handle_member_down_forced(dead_node).await
    }

    /// Triggers service duplication directly for all services configured on `node_id`,
    /// bypassing the leader election check (ideal for manual operator intervention or presentation demos).
    pub async fn duplicate_node_services(
        &self,
        node_id: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let dummy_node = PeerNode::new(
            node_id,
            "manual_demo",
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 8, 0, 99)),
            "127.0.0.1:0".parse().unwrap(),
        );
        self.handle_member_down_forced(&dummy_node).await
    }

    /// Internal execution loop duplicating matching services for a given dead/simulated node.
    pub async fn handle_member_down_forced(
        &self,
        dead_node: &PeerNode,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut spawned_count = 0;

        for service in &self.services {
            if service.node_id != dead_node.node_id {
                continue;
            }

            let Some(repl_cfg) = &service.replicate else {
                continue;
            };

            if !repl_cfg.enabled {
                continue;
            }

            let Some(host) = service.url.host_str() else {
                continue;
            };
            let domain = host.to_string();

            // Check if already duplicated
            if self.active_duplications.lock().await.contains_key(&domain) {
                continue;
            }

            let image = repl_cfg
                .image
                .clone()
                .unwrap_or_else(|| format!("{domain}:latest"));
            let port = repl_cfg
                .container_port
                .unwrap_or_else(|| service.url.port().unwrap_or(80));

            // Select replacement node
            let target_node_id = match repl_cfg.placement.as_str() {
                "leader" => self.local_node_id.clone(),
                explicit if explicit != "ring" && !explicit.is_empty() => {
                    let peers = self.cluster.peers.read().await;
                    if peers.contains_key(explicit) || explicit == self.local_node_id {
                        explicit.to_string()
                    } else {
                        self.select_ring_replacement(&domain).await
                    }
                }
                _ => self.select_ring_replacement(&domain).await,
            };

            info!(
                domain = %domain,
                dead_node = %dead_node.node_id,
                target_node = %target_node_id,
                "initiating service failover duplication"
            );

            // Record original route destination before failover
            let original_target = self.registry.lookup(&domain).map(|r| r.target_addr());

            if target_node_id == self.local_node_id {
                // Spawn locally on leader
                match self.driver.spawn_container(&domain, &image, &repl_cfg.env, port).await {
                    Ok(bound_addr) => {
                        let new_node = Node::new(&self.local_node_id, bound_addr);
                        let route = Route::new(Some(bound_addr), new_node);
                        self.registry.insert(domain.clone(), route.clone());
                        let _ = self.cluster.broadcast_route(domain.clone(), route).await;

                        self.active_duplications.lock().await.insert(
                            domain.clone(),
                            ActiveDuplication {
                                domain: domain.clone(),
                                origin_node_id: dead_node.node_id.clone(),
                                assigned_node_id: self.local_node_id.clone(),
                                failback_mode: repl_cfg.failback_mode,
                                failback_cooldown_secs: repl_cfg.failback_cooldown_secs,
                                spawned_addr: bound_addr,
                                original_target_addr: original_target,
                            },
                        );
                        spawned_count += 1;
                        info!(domain = %domain, %bound_addr, "successfully spawned duplicate service locally");
                    }
                    Err(err) => {
                        error!(domain = %domain, %err, "failed to spawn duplicate service locally");
                    }
                }
            } else {
                // Dispatch spawn command over WireGuard mesh to target node
                match self
                    .cluster
                    .send_spawn_workload(
                        &target_node_id,
                        domain.clone(),
                        image,
                        repl_cfg.env.clone(),
                        port,
                        dead_node.node_id.clone(),
                    )
                    .await
                {
                    Ok(_) => {
                        let placeholder_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
                        self.active_duplications.lock().await.insert(
                            domain.clone(),
                            ActiveDuplication {
                                domain: domain.clone(),
                                origin_node_id: dead_node.node_id.clone(),
                                assigned_node_id: target_node_id.clone(),
                                failback_mode: repl_cfg.failback_mode,
                                failback_cooldown_secs: repl_cfg.failback_cooldown_secs,
                                spawned_addr: placeholder_addr,
                                original_target_addr: original_target,
                            },
                        );
                        spawned_count += 1;
                        info!(domain = %domain, target = %target_node_id, "dispatched SpawnWorkload command to remote node");
                    }
                    Err(err) => {
                        error!(domain = %domain, target = %target_node_id, %err, "failed to dispatch SpawnWorkload command");
                    }
                }
            }
        }

        Ok(spawned_count)
    }

    /// Handles a node recovery event detected by SWIM.
    /// Evaluates `failback_mode` and initiates preemptive teardown after cooldown if configured.
    pub async fn handle_member_up(
        &self,
        recovered_node: &PeerNode,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        if !self.is_leader().await {
            return Ok(0);
        }

        let pending_dups: Vec<ActiveDuplication> = {
            let lock = self.active_duplications.lock().await;
            lock.values()
                .filter(|d| d.origin_node_id == recovered_node.node_id)
                .cloned()
                .collect()
        };

        let count = pending_dups.len();

        for dup in pending_dups {
            match dup.failback_mode {
                FailbackMode::NonPreemptive => {
                    info!(
                        domain = %dup.domain,
                        recovered_node = %recovered_node.node_id,
                        assigned_node = %dup.assigned_node_id,
                        "node recovered; non-preemptive failback mode maintaining duplicate workload"
                    );
                }
                FailbackMode::Manual => {
                    info!(
                        domain = %dup.domain,
                        recovered_node = %recovered_node.node_id,
                        assigned_node = %dup.assigned_node_id,
                        "node recovered; manual failback mode awaiting operator command"
                    );
                }
                FailbackMode::Preemptive => {
                    info!(
                        domain = %dup.domain,
                        recovered_node = %recovered_node.node_id,
                        cooldown_secs = dup.failback_cooldown_secs,
                        "node recovered; scheduling preemptive failback after cooldown"
                    );

                    let this = self.clone();
                    let origin_id = recovered_node.node_id.clone();
                    let domain = dup.domain.clone();
                    let cooldown = Duration::from_secs(dup.failback_cooldown_secs);

                    tokio::spawn(async move {
                        tokio::time::sleep(cooldown).await;
                        // Check if node is still alive after cooldown
                        let still_alive = {
                            let peers = this.cluster.peers.read().await;
                            peers.contains_key(&origin_id)
                        };

                        if still_alive {
                            let _ = this.execute_failback(&domain).await;
                        } else {
                            warn!(
                                domain = %domain,
                                node_id = %origin_id,
                                "recovered node flapped during cooldown; aborting preemptive failback"
                            );
                        }
                    });
                }
            }
        }

        Ok(count)
    }

    /// Executes failback for a domain: restores route to original node, stops duplicate, and removes record.
    pub async fn execute_failback(&self, domain: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let maybe_dup = {
            let mut lock = self.active_duplications.lock().await;
            lock.remove(domain)
        };

        let Some(dup) = maybe_dup else {
            return Ok(false);
        };

        info!(domain = %domain, origin = %dup.origin_node_id, assigned = %dup.assigned_node_id, "executing failback");

        // 1. Restore route pointing to recovered origin node
        let peers = self.cluster.peers.read().await;
        if let Some(peer) = peers.get(&dup.origin_node_id) {
            let restored_addr = dup.original_target_addr.unwrap_or(peer.endpoint);
            let restored_node = Node::new(&peer.node_id, restored_addr);
            let restored_route = Route::new(dup.original_target_addr, restored_node);
            self.registry.insert(domain.to_string(), restored_route.clone());
            let _ = self.cluster.broadcast_route(domain.to_string(), restored_route).await;
        }

        // 2. Stop duplicate workload
        if dup.assigned_node_id == self.local_node_id {
            let _ = self.driver.stop_container(domain).await;
        } else {
            let _ = self.cluster.send_stop_workload(&dup.assigned_node_id, domain.to_string()).await;
        }

        Ok(true)
    }

    /// Selects a replacement node for a domain using the consistent hash ring of surviving nodes.
    async fn select_ring_replacement(&self, domain: &str) -> String {
        let peers = self.cluster.peers.read().await;
        let mut healthy_nodes: Vec<String> = peers.keys().cloned().collect();
        healthy_nodes.push(self.local_node_id.clone());

        let ring = HashRing::from_nodes(healthy_nodes);
        ring.get(domain)
            .cloned()
            .unwrap_or_else(|| self.local_node_id.clone())
    }

    /// Runs the reactive event loop handling membership changes, remote workload commands, and shutdown.
    pub async fn run_with_shutdown(
        self: Arc<Self>,
        mut member_rx: broadcast::Receiver<MemberEvent>,
        mut command_rx: broadcast::Receiver<WorkloadCommand>,
        mut shutdown_rx: broadcast::Receiver<ShutdownReason>,
        ack_tx: Option<mpsc::Sender<ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            local_node = %self.local_node_id,
            monitored_services = self.services.len(),
            "service failover workload duplicator running"
        );

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("workload duplicator received shutdown signal");
                    break;
                }
                Ok(event) = member_rx.recv() => {
                    match event {
                        MemberEvent::Down(ref dead_node) => {
                            let _ = self.handle_member_down(dead_node).await;
                        }
                        MemberEvent::Up(ref recovered_node) => {
                            let _ = self.handle_member_up(recovered_node).await;
                        }
                    }
                }
                Ok(cmd) = command_rx.recv() => {
                    match cmd {
                        WorkloadCommand::Spawn { domain, image, env, container_port, origin_node: _, from_node } => {
                            info!(domain = %domain, from_leader = %from_node, "received remote SpawnWorkload command");
                            match self.driver.spawn_container(&domain, &image, &env, container_port).await {
                                Ok(bound_addr) => {
                                    let new_node = Node::new(&self.local_node_id, bound_addr);
                                    let route = Route::new(Some(bound_addr), new_node);
                                    self.registry.insert(domain.clone(), route.clone());
                                    let _ = self.cluster.broadcast_route(domain.clone(), route).await;
                                    let _ = self.cluster.send_workload_spawned(&from_node, domain, bound_addr).await;
                                }
                                Err(err) => {
                                    error!(domain = %domain, %err, "failed to execute remote SpawnWorkload");
                                }
                            }
                        }
                        WorkloadCommand::Stop { domain, from_node } => {
                            info!(domain = %domain, from_leader = %from_node, "received remote StopWorkload command");
                            let _ = self.driver.stop_container(&domain).await;
                        }
                        WorkloadCommand::Spawned { domain, target_addr, from_node } => {
                            info!(domain = %domain, spawned_node = %from_node, %target_addr, "remote node confirmed WorkloadSpawned");
                            let mut lock = self.active_duplications.lock().await;
                            if let Some(dup) = lock.get_mut(&domain) {
                                dup.spawned_addr = target_addr;
                            }
                        }
                    }
                }
            }
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(ShutdownAck {
                    subsystem: "service_failover".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}

/// Trait for triggering failover operations from control planes (IPC, CLI, Web UI).
pub trait FailoverTrigger: Send + Sync {
    fn trigger_replication<'a>(
        &'a self,
        node_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, String>> + Send + 'a>>;

    fn trigger_failback<'a>(
        &'a self,
        domain: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, String>> + Send + 'a>>;

    fn list_replicas<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ActiveDuplication>> + Send + 'a>>;
}

impl<D: ContainerDriver> FailoverTrigger for WorkloadDuplicator<D> {
    fn trigger_replication<'a>(
        &'a self,
        node_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<usize, String>> + Send + 'a>> {
        Box::pin(async move {
            self.duplicate_node_services(node_id)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn trigger_failback<'a>(
        &'a self,
        domain: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool, String>> + Send + 'a>> {
        Box::pin(async move {
            self.execute_failback(domain)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn list_replicas<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ActiveDuplication>> + Send + 'a>> {
        Box::pin(async move {
            self.active_duplications().await
        })
    }
}

