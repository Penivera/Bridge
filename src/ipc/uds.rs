use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use proxy::core::enums::ProxyMode;
use registry::{DomainRegistry, Node, Route};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;

use crate::failover::FailoverTrigger;

/// Inbound control requests sent over the Unix domain socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IpcRequest {
    Ping,
    Status,
    ListRoutes,
    AddRoute {
        domain: String,
        #[serde(default)]
        upstream: Option<SocketAddr>,
        #[serde(default)]
        node_id: Option<String>,
    },
    RemoveRoute {
        domain: String,
    },
    ClusterStatus,
    InspectDomain {
        domain: String,
        #[serde(default)]
        client_ip: Option<IpAddr>,
    },
    ListReplicas,
    TriggerReplication {
        node_id: String,
    },
    Failback {
        domain: String,
    },
    ListUsers,
    CreateUser {
        username: String,
        password: String,
    },
    SetPassword {
        username: String,
        password: String,
    },
    GetConfig,
    ReloadConfig,
    SetConfigKey {
        key: String,
        value: String,
    },
    MeshStatus,
    ElectionStatus,
    TriggerElection,
    SetLeader {
        node_id: String,
    },
    StepDown,
    DropNode {
        node_id: String,
    },
    PruneRoutes,
    MeshSync,
    ProxyStatus,
    Health,
    ListDiscovery,
}

/// Outbound control responses sent back over the Unix domain socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok {
        #[serde(flatten)]
        data: IpcData,
    },
    Error {
        message: String,
    },
}

/// Payload contained inside an `IpcResponse::Ok`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IpcData {
    Status {
        version: String,
        uptime_secs: u64,
        routes_count: usize,
        proxy_mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        local_node_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_leader: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        active_replicas_count: Option<usize>,
    },
    HealthSummary {
        system_health: String,
        uptime_secs: u64,
        cluster_health: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        local_node_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_leader: Option<bool>,
        total_nodes: usize,
        healthy_nodes: usize,
        total_routes: usize,
        healthy_routes: usize,
        active_replicas: usize,
    },
    ProxyStatus {
        mode: String,
        routes_count: usize,
        healthy_routes: usize,
        dead_routes: usize,
        uptime_secs: u64,
    },
    MeshDevice {
        interface_name: String,
        mesh_ip: IpAddr,
        public_key: String,
        listen_port: u16,
        peers_count: usize,
        peers: Vec<MeshPeerInfo>,
    },
    ElectionInfo {
        current_leader: Option<String>,
        term: u64,
        role: String,
        quorum_required: usize,
        total_known_nodes: usize,
        acks_count: usize,
        is_leader: bool,
    },
    ConfigView {
        config: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    Routes {
        routes: HashMap<String, RouteInfo>,
    },
    Cluster {
        local_node_id: String,
        current_leader: Option<String>,
        is_leader: bool,
        peers: Vec<PeerInfo>,
    },
    InspectResult {
        domain: String,
        targets: Vec<TargetInfo>,
        total_targets: usize,
        has_hash_ring: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_selected_target: Option<SocketAddr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_selected_node_id: Option<String>,
    },
    Replicas {
        replicas: Vec<ReplicaInfo>,
    },
    ReplicationResult {
        target_node: String,
        spawned_count: usize,
        message: String,
    },
    FailbackResult {
        domain: String,
        restored: bool,
        message: String,
    },
    RouteResult {
        domain: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        registered: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        removed: Option<bool>,
    },
    Pong {
        pong: bool,
        message: String,
    },
    Users {
        users: Vec<String>,
    },
    UserResult {
        username: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        created: Option<bool>,
        message: String,
    },
    AdminResult {
        action: String,
        target: String,
        message: String,
    },
    PruneResult {
        pruned_count: usize,
        pruned_domains: Vec<String>,
        message: String,
    },
    MeshSyncResult {
        synced_peers: usize,
        message: String,
    },
    DiscoveredServices {
        services: Vec<DiscoveredServiceInfo>,
    },
    Message {
        message: String,
    },
}

/// Serializable representation of an auto-discovered Docker container service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredServiceInfo {
    pub container_id: String,
    pub container_name: String,
    pub image: String,
    pub domains: Vec<String>,
    pub port: u16,
    pub upstream: Option<SocketAddr>,
    pub status: String,
}

/// Mesh peer info returned over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeshPeerInfo {
    pub public_key: String,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: u16,
}

/// Serializable representation of a route returned over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteInfo {
    pub upstream: Option<SocketAddr>,
    pub node_id: String,
    pub target_addr: SocketAddr,
    /// Liveness of the owning node: "healthy" or "dead".
    pub health: String,
}

impl From<&Route> for RouteInfo {
    fn from(r: &Route) -> Self {
        Self {
            upstream: r.upstream,
            node_id: r.node.node_id.clone(),
            target_addr: r.target_addr(),
            health: "unknown".to_string(),
        }
    }
}

/// Serializable representation of a cluster peer node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub node_id: String,
    pub mesh_ip: IpAddr,
    pub endpoint: SocketAddr,
    pub is_leader: bool,
    pub priority: u32,
}

/// Serializable representation of a backend target node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetInfo {
    pub node_id: String,
    pub address: SocketAddr,
    pub mesh_address: Option<SocketAddr>,
}

/// Serializable representation of an active duplicated service workload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicaInfo {
    pub domain: String,
    pub origin_node_id: String,
    pub assigned_node_id: String,
    pub failback_mode: String,
    pub failback_cooldown_secs: u64,
    pub spawned_addr: SocketAddr,
    pub original_target_addr: Option<SocketAddr>,
}

/// Shared runtime state for dashboard user management over IPC.
#[derive(Clone)]
pub struct UserManagementState {
    pub auth: Arc<RwLock<Option<Arc<crate::auth::AuthManager>>>>,
    pub config_json: Arc<RwLock<Option<serde_json::Value>>>,
    pub config_path: Arc<RwLock<Option<PathBuf>>>,
}

/// Unix Domain Socket IPC server for local host and container control.
pub struct IpcServer {
    socket_path: PathBuf,
    registry: Arc<DomainRegistry>,
    proxy_mode: ProxyMode,
    start_time: Instant,
    cluster: Arc<RwLock<Option<Arc<cluster::ClusterController>>>>,
    duplicator: Arc<RwLock<Option<Arc<dyn FailoverTrigger>>>>,
    auth: Arc<RwLock<Option<Arc<crate::auth::AuthManager>>>>,
    config_json: Arc<RwLock<Option<serde_json::Value>>>,
    config_path: Arc<RwLock<Option<PathBuf>>>,
    discovery_services: Arc<RwLock<Option<Arc<RwLock<HashMap<String, DiscoveredServiceInfo>>>>>>,
}

impl IpcServer {
    /// Creates a new IPC server configuration.
    pub fn new(
        socket_path: impl AsRef<Path>,
        registry: Arc<DomainRegistry>,
        proxy_mode: ProxyMode,
    ) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            registry,
            proxy_mode,
            start_time: Instant::now(),
            cluster: Arc::new(RwLock::new(None)),
            duplicator: Arc::new(RwLock::new(None)),
            auth: Arc::new(RwLock::new(None)),
            config_json: Arc::new(RwLock::new(None)),
            config_path: Arc::new(RwLock::new(None)),
            discovery_services: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns a shared handle to wire the `ClusterController` once initialized.
    pub fn cluster_handle(&self) -> Arc<RwLock<Option<Arc<cluster::ClusterController>>>> {
        self.cluster.clone()
    }

    /// Returns a shared handle to wire the `WorkloadDuplicator` once initialized.
    pub fn duplicator_handle(&self) -> Arc<RwLock<Option<Arc<dyn FailoverTrigger>>>> {
        self.duplicator.clone()
    }

    /// Returns a shared handle to wire the `AuthManager` once initialized.
    pub fn auth_handle(&self) -> Arc<RwLock<Option<Arc<crate::auth::AuthManager>>>> {
        self.auth.clone()
    }

    /// Returns a shared handle for the raw configuration document view.
    pub fn config_view_handle(&self) -> Arc<RwLock<Option<serde_json::Value>>> {
        self.config_json.clone()
    }

    /// Returns a shared handle for the configuration file path.
    pub fn config_path_handle(&self) -> Arc<RwLock<Option<PathBuf>>> {
        self.config_path.clone()
    }

    /// Returns a shared handle to wire discovered services once initialized.
    pub fn discovery_handle(&self) -> Arc<RwLock<Option<Arc<RwLock<HashMap<String, DiscoveredServiceInfo>>>>>> {
        self.discovery_services.clone()
    }

    /// Sets the shared handle for auto-discovered services.
    pub fn set_discovery_handle(
        &mut self,
        handle: Arc<RwLock<HashMap<String, DiscoveredServiceInfo>>>,
    ) {
        self.discovery_services = Arc::new(RwLock::new(Some(handle)));
    }

    /// Replaces the configuration view/path slots with shared ones (used by
    /// the daemon to keep the IPC user manager and dashboard in sync).
    pub fn set_config_slots(
        &mut self,
        json: Arc<RwLock<Option<serde_json::Value>>>,
        path: Arc<RwLock<Option<PathBuf>>>,
    ) {
        self.config_json = json;
        self.config_path = path;
    }

    /// Resolves and binds the Unix domain socket.
    ///
    /// Automatically removes stale socket files before binding and creates parent directories.
    pub fn bind(socket_path: impl AsRef<Path>) -> std::io::Result<UnixListener> {
        let path = socket_path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        UnixListener::bind(path)
    }

    /// Runs the IPC listener loop with a simple broadcast receiver.
    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            let _ = tx.send(proxy::ShutdownReason::Manual);
        });
        self.run_with_shutdown(rx, None).await
    }

    /// Runs the IPC listener loop coordinating with Bridge's ShutdownCoordinator.
    pub async fn run_with_shutdown(
        self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<proxy::ShutdownReason>,
        ack_tx: Option<tokio::sync::mpsc::Sender<proxy::ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (listener, active_path) = match Self::bind(&self.socket_path) {
            Ok(l) => (l, self.socket_path.clone()),
            Err(err) => {
                tracing::warn!(
                    path = %self.socket_path.display(),
                    %err,
                    "failed to bind preferred IPC socket, trying /tmp/bridge.sock fallback"
                );
                let fallback = PathBuf::from("/tmp/bridge.sock");
                let l = Self::bind(&fallback)?;
                (l, fallback)
            }
        };

        tracing::info!(
            socket = %active_path.display(),
            "IPC control server listening"
        );

        let registry = self.registry.clone();
        let proxy_mode = self.proxy_mode;
        let start_time = self.start_time;
        let cluster = self.cluster.clone();
        let duplicator = self.duplicator.clone();
        let auth = self.auth.clone();
        let config_json = self.config_json.clone();
        let config_path = self.config_path.clone();
        let discovery_services = self.discovery_services.clone();

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::info!("IPC server received shutdown signal");
                    break;
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _)) => {
                            let reg = registry.clone();
                            let c_slot = cluster.clone();
                            let d_slot = duplicator.clone();
                            let user_state = UserManagementState {
                                auth: auth.clone(),
                                config_json: config_json.clone(),
                                config_path: config_path.clone(),
                            };
                            let disc_slot = discovery_services.clone();
                            tokio::spawn(async move {
                                if let Err(err) = handle_ipc_connection(stream, reg, proxy_mode, start_time, c_slot, d_slot, user_state, disc_slot).await {
                                    tracing::debug!(%err, "IPC client connection closed with error");
                                }
                            });
                        }
                        Err(err) => {
                            tracing::warn!(%err, "error accepting IPC connection");
                        }
                    }
                }
            }
        }

        // Clean up socket file on graceful shutdown
        if active_path.exists() {
            let _ = std::fs::remove_file(&active_path);
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(proxy::ShutdownAck {
                    subsystem: "ipc_server".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}

async fn handle_ipc_connection(
    stream: UnixStream,
    registry: Arc<DomainRegistry>,
    proxy_mode: ProxyMode,
    start_time: Instant,
    cluster_slot: Arc<RwLock<Option<Arc<cluster::ClusterController>>>>,
    duplicator_slot: Arc<RwLock<Option<Arc<dyn FailoverTrigger>>>>,
    user_state: UserManagementState,
    discovery_slot: Arc<RwLock<Option<Arc<RwLock<HashMap<String, DiscoveredServiceInfo>>>>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<IpcRequest>(trimmed) {
            Ok(request) => match request {
                IpcRequest::Ping => IpcResponse::Ok {
                    data: IpcData::Pong {
                        pong: true,
                        message: "pong".to_string(),
                    },
                },
                IpcRequest::Status => {
                    let mut local_node_id = None;
                    let mut is_leader = None;
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        local_node_id = Some(ctrl.local_node.node_id.clone());
                        let election = ctrl.election_state.read().await;
                        is_leader = Some(
                            election
                                .current_leader
                                .as_ref()
                                .is_some_and(|l| l.node_id == ctrl.local_node.node_id),
                        );
                    }

                    let mut active_replicas_count = None;
                    let dup_guard = duplicator_slot.read().await;
                    if let Some(dup) = dup_guard.as_ref() {
                        active_replicas_count = Some(dup.list_replicas().await.len());
                    }

                    IpcResponse::Ok {
                        data: IpcData::Status {
                            version: env!("CARGO_PKG_VERSION").to_string(),
                            uptime_secs: start_time.elapsed().as_secs(),
                            routes_count: registry.len(),
                            proxy_mode: format!("{:?}", proxy_mode),
                            local_node_id,
                            is_leader,
                            active_replicas_count,
                        },
                    }
                }
                IpcRequest::ListRoutes => {
                    let snapshot = registry.snapshot();
                    let mut routes = HashMap::new();
                    let cluster_guard = cluster_slot.read().await;
                    for (k, v) in snapshot.iter() {
                        let mut info = RouteInfo::from(v);
                        if let Some(ctrl) = cluster_guard.as_ref() {
                            let alive = ctrl.local_node.node_id == v.node.node_id
                                || ctrl.peers.read().await.contains_key(&v.node.node_id);
                            info.health = if alive { "healthy".to_string() } else { "dead".to_string() };
                        }
                        routes.insert(k.clone(), info);
                    }
                    IpcResponse::Ok {
                        data: IpcData::Routes { routes },
                    }
                }
                IpcRequest::ListDiscovery => {
                    let disc_guard = discovery_slot.read().await;
                    let services = match disc_guard.as_ref() {
                        Some(handle) => handle.read().await.values().cloned().collect(),
                        None => Vec::new(),
                    };
                    IpcResponse::Ok {
                        data: IpcData::DiscoveredServices { services },
                    }
                }
                IpcRequest::AddRoute {
                    domain,
                    upstream,
                    node_id,
                } => {
                    let nid = node_id.unwrap_or_else(|| "self".to_string());
                    let target_addr = upstream.unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 80)));
                    let node = Node::new(nid, target_addr);
                    let route = Route::new(upstream, node);
                    registry.insert(domain.clone(), route);
                    IpcResponse::Ok {
                        data: IpcData::RouteResult {
                            domain,
                            registered: Some(true),
                            removed: None,
                        },
                    }
                }
                IpcRequest::RemoveRoute { domain } => {
                    let removed = registry.remove(&domain);
                    IpcResponse::Ok {
                        data: IpcData::RouteResult {
                            domain,
                            registered: None,
                            removed: Some(removed.is_some()),
                        },
                    }
                }
                IpcRequest::ClusterStatus => {
                    let cluster_guard = cluster_slot.read().await;
                    match cluster_guard.as_ref() {
                        Some(ctrl) => {
                            let election = ctrl.election_state.read().await;
                            let leader_node_id = election.current_leader.as_ref().map(|l| l.node_id.clone());
                            let is_leader = leader_node_id.as_deref() == Some(&ctrl.local_node.node_id);

                            let mut peers = Vec::new();
                            // Add self
                            peers.push(PeerInfo {
                                node_id: ctrl.local_node.node_id.clone(),
                                mesh_ip: ctrl.local_node.mesh_ip,
                                endpoint: ctrl.local_node.endpoint,
                                is_leader,
                                priority: ctrl.local_node.priority,
                            });

                            // Add remote peers
                            let peers_map = ctrl.peers.read().await;
                            for p in peers_map.values() {
                                let peer_is_leader = leader_node_id.as_deref() == Some(&p.node_id);
                                peers.push(PeerInfo {
                                    node_id: p.node_id.clone(),
                                    mesh_ip: p.mesh_ip,
                                    endpoint: p.endpoint,
                                    is_leader: peer_is_leader,
                                    priority: p.priority,
                                });
                            }

                            IpcResponse::Ok {
                                data: IpcData::Cluster {
                                    local_node_id: ctrl.local_node.node_id.clone(),
                                    current_leader: leader_node_id,
                                    is_leader,
                                    peers,
                                },
                            }
                        }
                        None => IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node".to_string(),
                        },
                    }
                }
                IpcRequest::InspectDomain { domain, client_ip } => {
                    match registry.lookup(&domain) {
                        Some(route) => {
                            let targets: Vec<TargetInfo> = route
                                .targets
                                .iter()
                                .map(|n| TargetInfo {
                                    node_id: n.node_id.clone(),
                                    address: n.address,
                                    mesh_address: n.mesh_address,
                                })
                                .collect();

                            let has_hash_ring = route.ring.is_some();
                            let (client_selected_target, client_selected_node_id) =
                                if let Some(ip) = client_ip {
                                    let target = route.select_node_for_client(ip, &domain);
                                    (Some(target.address), Some(target.node_id.clone()))
                                } else {
                                    (None, None)
                                };

                            IpcResponse::Ok {
                                data: IpcData::InspectResult {
                                    domain,
                                    total_targets: targets.len(),
                                    targets,
                                    has_hash_ring,
                                    client_selected_target,
                                    client_selected_node_id,
                                },
                            }
                        }
                        None => IpcResponse::Error {
                            message: format!("Route for domain '{domain}' not found in registry"),
                        },
                    }
                }
                IpcRequest::ListReplicas => {
                    let dup_guard = duplicator_slot.read().await;
                    let replicas = match dup_guard.as_ref() {
                        Some(dup) => dup
                            .list_replicas()
                            .await
                            .into_iter()
                            .map(|d| ReplicaInfo {
                                domain: d.domain,
                                origin_node_id: d.origin_node_id,
                                assigned_node_id: d.assigned_node_id,
                                failback_mode: format!("{:?}", d.failback_mode),
                                failback_cooldown_secs: d.failback_cooldown_secs,
                                spawned_addr: d.spawned_addr,
                                original_target_addr: d.original_target_addr,
                            })
                            .collect(),
                        None => Vec::new(),
                    };

                    IpcResponse::Ok {
                        data: IpcData::Replicas { replicas },
                    }
                }
                IpcRequest::TriggerReplication { node_id } => {
                    let dup_guard = duplicator_slot.read().await;
                    match dup_guard.as_ref() {
                        Some(dup) => match dup.trigger_replication(&node_id).await {
                            Ok(count) => IpcResponse::Ok {
                                data: IpcData::ReplicationResult {
                                    target_node: node_id.clone(),
                                    spawned_count: count,
                                    message: format!("Successfully spawned {count} replica service(s) for node '{node_id}'"),
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to replicate services for node '{node_id}': {err}"),
                            },
                        },
                        None => IpcResponse::Error {
                            message: "Service failover duplicator is not active on this node".to_string(),
                        },
                    }
                }
                IpcRequest::Failback { domain } => {
                    let dup_guard = duplicator_slot.read().await;
                    match dup_guard.as_ref() {
                        Some(dup) => match dup.trigger_failback(&domain).await {
                            Ok(true) => IpcResponse::Ok {
                                data: IpcData::FailbackResult {
                                    domain: domain.clone(),
                                    restored: true,
                                    message: format!("Failback executed. Service '{domain}' restored to origin node."),
                                },
                            },
                            Ok(false) => IpcResponse::Error {
                                message: format!("No active replica found for domain '{domain}'"),
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to execute failback for domain '{domain}': {err}"),
                            },
                        },
                        None => IpcResponse::Error {
                            message: "Service failover duplicator is not active on this node".to_string(),
                        },
                    }
                }
                IpcRequest::ListUsers => {
                    let auth_guard = user_state.auth.read().await;
                    match auth_guard.as_ref() {
                        Some(auth) => IpcResponse::Ok {
                            data: IpcData::Users {
                                users: auth.list_users().await,
                            },
                        },
                        None => IpcResponse::Error {
                            message: "Authentication manager is not active on this node".to_string(),
                        },
                    }
                }
                IpcRequest::CreateUser { username, password } => {
                    match handle_user_upsert(&user_state, &username, &password, false).await {
                        Ok(created) => IpcResponse::Ok {
                            data: IpcData::UserResult {
                                username: username.clone(),
                                created: Some(created),
                                message: format!("User '{username}' created"),
                            },
                        },
                        Err(message) => IpcResponse::Error { message },
                    }
                }
                IpcRequest::SetPassword { username, password } => {
                    match handle_user_upsert(&user_state, &username, &password, true).await {
                        Ok(_) => IpcResponse::Ok {
                            data: IpcData::UserResult {
                                username: username.clone(),
                                created: None,
                                message: format!("Password updated for user '{username}'"),
                            },
                        },
                        Err(message) => IpcResponse::Error { message },
                    }
                }
                IpcRequest::GetConfig => {
                    let p_guard = user_state.config_path.read().await;
                    let path = p_guard.clone();
                    let j_guard = user_state.config_json.read().await;
                    let json_val = if let Some(v) = j_guard.as_ref() {
                        v.clone()
                    } else if let Some(p) = &path {
                        if let Ok(raw) = std::fs::read_to_string(p) {
                            let is_yaml = matches!(p.extension().and_then(|e| e.to_str()), Some("yaml" | "yml"));
                            if is_yaml {
                                serde_yaml::from_str(&raw).unwrap_or(serde_json::Value::Null)
                            } else {
                                toml::from_str(&raw).unwrap_or(serde_json::Value::Null)
                            }
                        } else {
                            serde_json::Value::Null
                        }
                    } else {
                        serde_json::Value::Null
                    };
                    IpcResponse::Ok {
                        data: IpcData::ConfigView {
                            config: json_val,
                            path: path.map(|p| p.display().to_string()),
                        },
                    }
                }
                IpcRequest::ReloadConfig => {
                    let p_guard = user_state.config_path.read().await;
                    if let Some(path) = p_guard.clone() {
                        let load_result = crate::core::config::Config::load_auto(Some(&path)).map_err(|e| e.to_string());
                        match load_result {
                            Ok(cfg) => {
                                let node_map: HashMap<&String, &Node> = cfg.nodes.iter().map(|n| (&n.node_id, n)).collect();
                                let new_routes = crate::daemon::build_initial_routes(&cfg, &node_map);
                                for (dom, route) in new_routes {
                                    registry.insert_versioned(dom, route);
                                }
                                if let Some(view) = crate::daemon::load_config_view(&cfg) {
                                    *user_state.config_json.write().await = Some(view);
                                }
                                IpcResponse::Ok {
                                    data: IpcData::Message {
                                        message: format!("Successfully reloaded configuration from {}", path.display()),
                                    },
                                }
                            }
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to reload config from {}: {err}", path.display()),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "No configuration file loaded on daemon; cannot reload".to_string(),
                        }
                    }
                }
                IpcRequest::SetConfigKey { key, value } => {
                    match handle_config_set(&user_state, &key, &value).await {
                        Ok(msg) => IpcResponse::Ok {
                            data: IpcData::Message { message: msg },
                        },
                        Err(err) => IpcResponse::Error { message: err },
                    }
                }
                IpcRequest::MeshStatus => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        let wg_guard = ctrl.wireguard.read().await;
                        let peers: Vec<MeshPeerInfo> = wg_guard
                            .list_peers()
                            .into_iter()
                            .map(|p| MeshPeerInfo {
                                public_key: p.public_key,
                                endpoint: p.endpoint,
                                allowed_ips: p.allowed_ips,
                                persistent_keepalive: p.persistent_keepalive,
                            })
                            .collect();
                        IpcResponse::Ok {
                            data: IpcData::MeshDevice {
                                interface_name: wg_guard.interface_name.clone(),
                                mesh_ip: wg_guard.mesh_ip,
                                public_key: wg_guard.public_key.clone(),
                                listen_port: wg_guard.listen_port,
                                peers_count: peers.len(),
                                peers,
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "WireGuard cluster mesh not active on this node (standalone mode)".to_string(),
                        }
                    }
                }
                IpcRequest::ElectionStatus => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        let election = ctrl.election_state.read().await;
                        let leader_id = election.current_leader.as_ref().map(|l| l.node_id.clone());
                        let is_leader = leader_id.as_deref() == Some(&ctrl.local_node.node_id);
                        let quorum = election.quorum_required();
                        let total = election.total_known_nodes;
                        let acks_count = election.acks.len();
                        let role = format!("{:?}", election.role);
                        let term = election.term;

                        IpcResponse::Ok {
                            data: IpcData::ElectionInfo {
                                current_leader: leader_id,
                                term,
                                role,
                                quorum_required: quorum,
                                total_known_nodes: total,
                                acks_count,
                                is_leader,
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Leader election not active on this node (standalone mode)".to_string(),
                        }
                    }
                }
                IpcRequest::TriggerElection => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        match ctrl.start_election().await {
                            Ok(won) => IpcResponse::Ok {
                                data: IpcData::Message {
                                    message: if won {
                                        "Election initiated: local node declared coordinator (leader)".to_string()
                                    } else {
                                        "Election initiated: candidate challenge sent across cluster".to_string()
                                    },
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to initiate election: {err}"),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node".to_string(),
                        }
                    }
                }
                IpcRequest::ProxyStatus => {
                    let total_routes = registry.len();
                    let snapshot = registry.snapshot();
                    let cluster_guard = cluster_slot.read().await;
                    let mut healthy_routes = 0;
                    let mut dead_routes = 0;
                    for (_, r) in snapshot.iter() {
                        let alive = if let Some(ctrl) = cluster_guard.as_ref() {
                            ctrl.local_node.node_id == r.node.node_id
                                || ctrl.peers.read().await.contains_key(&r.node.node_id)
                        } else {
                            true
                        };
                        if alive { healthy_routes += 1; } else { dead_routes += 1; }
                    }
                    IpcResponse::Ok {
                        data: IpcData::ProxyStatus {
                            mode: format!("{:?}", proxy_mode),
                            routes_count: total_routes,
                            healthy_routes,
                            dead_routes,
                            uptime_secs: start_time.elapsed().as_secs(),
                        },
                    }
                }
                IpcRequest::Health => {
                    let total_routes = registry.len();
                    let snapshot = registry.snapshot();
                    let cluster_guard = cluster_slot.read().await;
                    let mut healthy_routes = 0;
                    let mut dead_routes = 0;
                    let mut cluster_health = "standalone".to_string();
                    let mut total_nodes = 1;
                    let mut healthy_nodes = 1;
                    let mut local_node_id = None;
                    let mut is_leader = None;

                    if let Some(ctrl) = cluster_guard.as_ref() {
                        local_node_id = Some(ctrl.local_node.node_id.clone());
                        let election = ctrl.election_state.read().await;
                        is_leader = Some(election.current_leader.as_ref().is_some_and(|l| l.node_id == ctrl.local_node.node_id));
                        let peers_map = ctrl.peers.read().await;
                        total_nodes = 1 + peers_map.len();
                        healthy_nodes = 1 + peers_map.len();
                        cluster_health = if election.current_leader.is_some() {
                            "healthy".to_string()
                        } else {
                            "electing".to_string()
                        };
                        for (_, r) in snapshot.iter() {
                            let alive = ctrl.local_node.node_id == r.node.node_id
                                || peers_map.contains_key(&r.node.node_id);
                            if alive { healthy_routes += 1; } else { dead_routes += 1; }
                        }
                    } else {
                        healthy_routes = total_routes;
                    }

                    let active_replicas = if let Some(dup) = duplicator_slot.read().await.as_ref() {
                        dup.list_replicas().await.len()
                    } else {
                        0
                    };

                    let status = if dead_routes > 0 || cluster_health == "electing" {
                        "degraded".to_string()
                    } else {
                        "healthy".to_string()
                    };

                    IpcResponse::Ok {
                        data: IpcData::HealthSummary {
                            system_health: status,
                            uptime_secs: start_time.elapsed().as_secs(),
                            cluster_health,
                            local_node_id,
                            is_leader,
                            total_nodes,
                            healthy_nodes,
                            total_routes,
                            healthy_routes,
                            active_replicas,
                        },
                    }
                }
                IpcRequest::SetLeader { node_id } => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        match ctrl.force_leader(&node_id).await {
                            Ok((leader, term)) => IpcResponse::Ok {
                                data: IpcData::AdminResult {
                                    action: "set_leader".to_string(),
                                    target: leader.node_id.clone(),
                                    message: format!("Leader administratively set to '{}' at term {}", leader.node_id, term),
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to set leader: {err}"),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node (standalone mode)".to_string(),
                        }
                    }
                }
                IpcRequest::StepDown => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        match ctrl.step_down().await {
                            Ok(stepped_down) => IpcResponse::Ok {
                                data: IpcData::AdminResult {
                                    action: "step_down".to_string(),
                                    target: ctrl.local_node.node_id.clone(),
                                    message: if stepped_down {
                                        format!("Node '{}' successfully stepped down from leadership", ctrl.local_node.node_id)
                                    } else {
                                        format!("Node '{}' was not the leader, election initiated", ctrl.local_node.node_id)
                                    },
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to step down: {err}"),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node (standalone mode)".to_string(),
                        }
                    }
                }
                IpcRequest::DropNode { node_id } => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        match ctrl.drop_node(&node_id).await {
                            Ok(dropped) => IpcResponse::Ok {
                                data: IpcData::AdminResult {
                                    action: "drop_node".to_string(),
                                    target: dropped.node_id.clone(),
                                    message: format!("Node '{}' successfully dropped and evicted from cluster", dropped.node_id),
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to drop node: {err}"),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node (standalone mode)".to_string(),
                        }
                    }
                }
                IpcRequest::PruneRoutes => {
                    let cluster_guard = cluster_slot.read().await;
                    let snapshot = registry.snapshot();
                    let mut pruned = Vec::new();
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        let peers_map = ctrl.peers.read().await;
                        for (domain, route) in snapshot.iter() {
                            let alive = ctrl.local_node.node_id == route.node.node_id
                                || peers_map.contains_key(&route.node.node_id);
                            if !alive {
                                registry.remove(domain);
                                pruned.push(domain.clone());
                            }
                        }
                    }
                    let count = pruned.len();
                    IpcResponse::Ok {
                        data: IpcData::PruneResult {
                            pruned_count: count,
                            pruned_domains: pruned,
                            message: format!("Pruned {count} stale route(s) from registry"),
                        },
                    }
                }
                IpcRequest::MeshSync => {
                    let cluster_guard = cluster_slot.read().await;
                    if let Some(ctrl) = cluster_guard.as_ref() {
                        match ctrl.sync_wireguard_kernel().await {
                            Ok(synced_peers) => IpcResponse::Ok {
                                data: IpcData::MeshSyncResult {
                                    synced_peers,
                                    message: format!("Synchronized {synced_peers} WireGuard peer(s) to kernel"),
                                },
                            },
                            Err(err) => IpcResponse::Error {
                                message: format!("Failed to sync WireGuard kernel interface: {err}"),
                            },
                        }
                    } else {
                        IpcResponse::Error {
                            message: "Cluster mesh not enabled or active on this node (standalone mode)".to_string(),
                        }
                    }
                }
            },
            Err(err) => IpcResponse::Error {
                message: format!("invalid JSON request: {err}"),
            },
        };

        let mut serialized = serde_json::to_string(&response)?;
        serialized.push('\n');
        writer.write_all(serialized.as_bytes()).await?;
        writer.flush().await?;
    }

    Ok(())
}

/// Hashes a password, updates the config file's `[auth]` section, and refreshes
/// the runtime auth manager. Returns whether the user was newly created.
async fn handle_user_upsert(
    user_state: &UserManagementState,
    username: &str,
    password: &str,
    must_exist: bool,
) -> Result<bool, String> {
    use crate::auth::AuthManager;

    crate::auth::validate_username(username).map_err(|e| e.to_string())?;
    let hash = AuthManager::hash_password(password).map_err(|e| e.to_string())?;

    let auth = {
        let guard = user_state.auth.read().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "Authentication manager is not active on this node".to_string())?
    };

    if must_exist {
        let exists = auth.list_users().await.iter().any(|u| u == username);
        if !exists {
            return Err(format!("user '{username}' does not exist"));
        }
    } else {
        let exists = auth.list_users().await.iter().any(|u| u == username);
        if exists {
            return Err(format!(
                "user '{username}' already exists; use 'bridge user passwd' to change the password"
            ));
        }
    }

    // Load the current configuration document (runtime view or the file itself).
    let path = {
        let guard = user_state.config_path.read().await;
        guard
            .clone()
            .ok_or_else(|| "no configuration file loaded; cannot persist users".to_string())?
    };

    let mut doc = {
        let guard = user_state.config_json.read().await;
        if let Some(view) = guard.as_ref() {
            view.clone()
        } else {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read config file {}: {e}", path.display()))?;
            let is_yaml = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("yaml" | "yml")
            );
            if is_yaml {
                serde_yaml::from_str(&raw).map_err(|e| format!("invalid YAML config: {e}"))?
            } else {
                toml::from_str(&raw).map_err(|e| format!("invalid TOML config: {e}"))?
            }
        }
    };

    let (updated, created) = crate::auth::apply_user_to_config_doc(doc, username, &hash);
    doc = updated;

    let is_yaml = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml")
    );
    let serialized = if is_yaml {
        serde_yaml::to_string(&doc).map_err(|e| format!("cannot encode YAML: {e}"))?
    } else {
        toml::to_string(&doc).map_err(|e| format!("cannot encode TOML: {e}"))?
    };

    // Reject documents the daemon itself would refuse to load.
    if is_yaml {
        crate::core::config::Config::from_yaml_str(&serialized)
            .map_err(|e| format!("invalid configuration: {e}"))?;
    } else {
        crate::core::config::Config::from_toml_str(&serialized)
            .map_err(|e| format!("invalid configuration: {e}"))?;
    }

    // Atomic-ish write: same-directory temp file + rename.
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &serialized)
        .map_err(|e| format!("failed to write config: {e}"))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("failed to replace config: {e}")
    })?;

    *user_state.config_json.write().await = Some(doc);

    auth.upsert_user(username, hash)
        .await
        .map_err(|e| e.to_string())?;

    Ok(created)
}

/// Updates a configuration setting, persists it to the config file on disk,
/// and updates the runtime config view.
async fn handle_config_set(
    user_state: &UserManagementState,
    key: &str,
    value: &str,
) -> Result<String, String> {
    let path = {
        let guard = user_state.config_path.read().await;
        guard
            .clone()
            .ok_or_else(|| "no configuration file loaded; cannot update settings".to_string())?
    };

    let mut doc = {
        let guard = user_state.config_json.read().await;
        if let Some(view) = guard.as_ref() {
            view.clone()
        } else {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read config file {}: {e}", path.display()))?;
            let is_yaml = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("yaml" | "yml")
            );
            if is_yaml {
                serde_yaml::from_str(&raw).map_err(|e| format!("invalid YAML config: {e}"))?
            } else {
                toml::from_str(&raw).map_err(|e| format!("invalid TOML config: {e}"))?
            }
        }
    };

    let parsed_val: serde_json::Value = if value.eq_ignore_ascii_case("true") {
        serde_json::Value::Bool(true)
    } else if value.eq_ignore_ascii_case("false") {
        serde_json::Value::Bool(false)
    } else if let Ok(n) = value.parse::<i64>() {
        serde_json::Value::Number(n.into())
    } else {
        serde_json::Value::String(value.to_string())
    };

    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        return Err("empty configuration key".to_string());
    }

    let mut current = &mut doc;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.insert((*part).to_string(), parsed_val.clone());
            } else {
                return Err(format!("cannot set '{key}': parent is not an object"));
            }
        } else {
            if !current.is_object() {
                *current = serde_json::Value::Object(serde_json::Map::new());
            }
            current = current
                .as_object_mut()
                .unwrap()
                .entry((*part).to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        }
    }

    let is_yaml = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml")
    );
    let serialized = if is_yaml {
        serde_yaml::to_string(&doc).map_err(|e| format!("cannot encode YAML: {e}"))?
    } else {
        toml::to_string(&doc).map_err(|e| format!("cannot encode TOML: {e}"))?
    };

    if is_yaml {
        crate::core::config::Config::from_yaml_str(&serialized)
            .map_err(|e| format!("invalid configuration after change: {e}"))?;
    } else {
        crate::core::config::Config::from_toml_str(&serialized)
            .map_err(|e| format!("invalid configuration after change: {e}"))?;
    }

    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &serialized)
        .map_err(|e| format!("failed to write config: {e}"))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("failed to replace config: {e}")
    })?;

    *user_state.config_json.write().await = Some(doc);
    Ok(format!("Configuration updated: {key} = {value} ({})", path.display()))
}

/// Client for connecting to the Bridge IPC Unix domain socket.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// Connects to a Bridge IPC server listening on `socket_path`.
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let stream = UnixStream::connect(socket_path).await?;
        Ok(Self { stream })
    }

    /// Sends a typed `IpcRequest` and receives an `IpcResponse`.
    pub async fn send(
        &mut self,
        request: &IpcRequest,
    ) -> Result<IpcResponse, Box<dyn std::error::Error>> {
        let mut req_str = serde_json::to_string(request)?;
        req_str.push('\n');
        self.stream.write_all(req_str.as_bytes()).await?;
        self.stream.flush().await?;

        let mut reader = BufReader::new(&mut self.stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: IpcResponse = serde_json::from_str(line.trim())?;
        Ok(response)
    }

    /// Sends a `ping` and returns the pong response string.
    pub async fn ping(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::Ping).await? {
            IpcResponse::Ok {
                data: IpcData::Pong { message, .. },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves status from the running daemon.
    pub async fn status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::Status).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Lists all routes from the running daemon.
    pub async fn list_routes(
        &mut self,
    ) -> Result<HashMap<String, RouteInfo>, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ListRoutes).await? {
            IpcResponse::Ok {
                data: IpcData::Routes { routes },
            } => Ok(routes),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Adds a route dynamically.
    pub async fn add_route(
        &mut self,
        domain: impl Into<String>,
        upstream: Option<SocketAddr>,
        node_id: Option<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::AddRoute {
                domain: domain.into(),
                upstream,
                node_id,
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::RouteResult { registered, .. },
            } => Ok(registered.unwrap_or(true)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Removes a route dynamically.
    pub async fn remove_route(
        &mut self,
        domain: impl Into<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::RemoveRoute {
                domain: domain.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::RouteResult { removed, .. },
            } => Ok(removed.unwrap_or(false)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves cluster membership, leader status, and mesh peers.
    pub async fn cluster_status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ClusterStatus).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Inspects domain route targets, consistent hash ring status, and composite affinity.
    pub async fn inspect_domain(
        &mut self,
        domain: impl Into<String>,
        client_ip: Option<IpAddr>,
    ) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::InspectDomain {
                domain: domain.into(),
                client_ip,
            })
            .await?
        {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Lists active duplicated workloads.
    pub async fn list_replicas(&mut self) -> Result<Vec<ReplicaInfo>, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ListReplicas).await? {
            IpcResponse::Ok {
                data: IpcData::Replicas { replicas },
            } => Ok(replicas),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Triggers manual service replication for a node.
    pub async fn trigger_replication(
        &mut self,
        node_id: impl Into<String>,
    ) -> Result<(usize, String), Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::TriggerReplication {
                node_id: node_id.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data:
                    IpcData::ReplicationResult {
                        spawned_count,
                        message,
                        ..
                    },
            } => Ok((spawned_count, message)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Triggers manual failback for a duplicated service.
    pub async fn failback(
        &mut self,
        domain: impl Into<String>,
    ) -> Result<(bool, String), Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::Failback {
                domain: domain.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::FailbackResult { restored, message, .. },
            } => Ok((restored, message)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Lists configured dashboard users.
    pub async fn list_users(&mut self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ListUsers).await? {
            IpcResponse::Ok {
                data: IpcData::Users { users },
            } => Ok(users),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Creates a dashboard user with the given password.
    pub async fn create_user(
        &mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::CreateUser {
                username: username.into(),
                password: password.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::UserResult { message, .. },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Updates a dashboard user's password.
    pub async fn set_password(
        &mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::SetPassword {
                username: username.into(),
                password: password.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::UserResult { message, .. },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves the runtime configuration document from the daemon.
    pub async fn get_config(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::GetConfig).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Triggers configuration reload from disk on the running daemon.
    pub async fn reload_config(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ReloadConfig).await? {
            IpcResponse::Ok {
                data: IpcData::Message { message },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Sets a configuration key and persists it to disk.
    pub async fn set_config_key(
        &mut self,
        key: &str,
        value: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::SetConfigKey {
                key: key.to_string(),
                value: value.to_string(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::Message { message },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves WireGuard mesh device info and configured peers.
    pub async fn mesh_status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::MeshStatus).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Retrieves Bully leader election state.
    pub async fn election_status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ElectionStatus).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Triggers a leadership election cycle or step-down.
    pub async fn trigger_election(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::TriggerElection).await? {
            IpcResponse::Ok {
                data: IpcData::Message { message },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves proxy engine status, mode, and target routing counts.
    pub async fn proxy_status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ProxyStatus).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Retrieves system health summary.
    pub async fn health(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::Health).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Administratively force specifies a leader on the cluster.
    pub async fn set_leader(&mut self, node_id: String) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::SetLeader { node_id }).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Administratively steps down from leadership on this node.
    pub async fn step_down(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::StepDown).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Administratively drops/evicts a node from the cluster and mesh.
    pub async fn drop_node(&mut self, node_id: String) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::DropNode { node_id }).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Administratively prunes stale/orphaned routes from the registry.
    pub async fn prune_routes(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::PruneRoutes).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Forces a sync of in-memory WireGuard peers to the Linux kernel interface.
    pub async fn mesh_sync(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::MeshSync).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Lists auto-discovered Docker container services.
    pub async fn list_discovery(
        &mut self,
    ) -> Result<Vec<DiscoveredServiceInfo>, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ListDiscovery).await? {
            IpcResponse::Ok {
                data: IpcData::DiscoveredServices { services },
            } => Ok(services),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }
}
