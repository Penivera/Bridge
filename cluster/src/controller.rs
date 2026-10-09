use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use foca::{
    AccumulatingRuntime, Config as FocaConfig, Foca, NoCustomBroadcast, OwnedNotification,
    PostcardCodec, Timer,
};
use mesh::wireguard::{WireGuardDevice, WireGuardPeer};
use rand::rngs::StdRng;
use registry::{DomainRegistry, Node, Route};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::election::{ElectionRole, ElectionState};
use crate::protocol::{ClusterMessage, PeerNode};

/// Cluster membership event emitted by the SWIM failure detector.
#[derive(Debug, Clone)]
pub enum MemberEvent {
    Up(PeerNode),
    Down(PeerNode),
}

/// Workload command dispatched by the cluster leader for service failover duplication.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WorkloadCommand {
    Spawn {
        from_node: String,
        domain: String,
        image: String,
        env: Vec<String>,
        container_port: u16,
        origin_node: String,
    },
    Stop {
        from_node: String,
        domain: String,
    },
    Spawned {
        from_node: String,
        domain: String,
        target_addr: SocketAddr,
    },
}

/// Controller managing the raw UDP cluster protocol, peer bootstrap, WireGuard sync, SWIM failure detection,
/// and Bully leader election with quorum consensus.
#[derive(Clone)]
pub struct ClusterController {
    pub local_node: PeerNode,
    pub socket: Arc<UdpSocket>,
    pub peers: Arc<RwLock<HashMap<String, PeerNode>>>,
    pub wireguard: Arc<RwLock<WireGuardDevice>>,
    pub registry: Arc<DomainRegistry>,
    pub foca: Arc<Mutex<Foca<PeerNode, PostcardCodec, StdRng, NoCustomBroadcast>>>,
    pub gossip_period: Duration,
    timer_tx: mpsc::Sender<Timer<PeerNode>>,
    timer_rx: Arc<Mutex<mpsc::Receiver<Timer<PeerNode>>>>,
    pub election_state: Arc<RwLock<ElectionState>>,
    leader_tx: Arc<watch::Sender<Option<PeerNode>>>,
    leader_rx: watch::Receiver<Option<PeerNode>>,
    membership_tx: Arc<tokio::sync::broadcast::Sender<MemberEvent>>,
    workload_tx: Arc<tokio::sync::broadcast::Sender<WorkloadCommand>>,
    pub election_timeout: Duration,
    pub ack_timeout: Duration,
    /// Domains that must never leave this node (excluded from route gossip).
    /// Used for node-local services such as the leader-only dashboard ingress.
    pub local_only_domains: Arc<RwLock<HashSet<String>>>,
    /// Configured bootstrap seed endpoints; periodically re-contacted while no
    /// peers are known so simultaneously-booted fleets still converge.
    seeds: Arc<RwLock<Vec<SocketAddr>>>,
}

impl ClusterController {
    /// Creates a new `ClusterController` bound to the local node's cluster endpoint.
    pub async fn bind(
        local_node: PeerNode,
        wireguard: Arc<RwLock<WireGuardDevice>>,
        registry: Arc<DomainRegistry>,
    ) -> std::io::Result<Self> {
        // Bind the wildcard address so NAT'd nodes (which advertise a public
        // endpoint they do not own locally) can still join. The advertised
        // endpoint stays `local_node.endpoint`.
        let bind_addr = SocketAddr::new(
            if local_node.endpoint.is_ipv4() {
                std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
            } else {
                std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED)
            },
            local_node.endpoint.port(),
        );
        let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
        let foca = Arc::new(Mutex::new(Self::init_foca(&local_node, None)));
        let (timer_tx, timer_rx) = mpsc::channel(256);
        let (leader_tx, leader_rx) = watch::channel(None);
        let (membership_tx, _) = tokio::sync::broadcast::channel(64);
        let (workload_tx, _) = tokio::sync::broadcast::channel(64);
        let election_state = Arc::new(RwLock::new(ElectionState::new()));
        Ok(Self {
            local_node,
            socket,
            peers: Arc::new(RwLock::new(HashMap::new())),
            wireguard,
            registry,
            foca,
            gossip_period: Duration::from_millis(500),
            timer_tx,
            timer_rx: Arc::new(Mutex::new(timer_rx)),
            election_state,
            leader_tx: Arc::new(leader_tx),
            leader_rx,
            membership_tx: Arc::new(membership_tx),
            workload_tx: Arc::new(workload_tx),
            election_timeout: Duration::from_millis(300),
            ack_timeout: Duration::from_millis(300),
            local_only_domains: Arc::new(RwLock::new(HashSet::new())),
            seeds: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Creates a controller with an existing bound `UdpSocket` (convenient for testing).
    pub fn new_with_socket(
        local_node: PeerNode,
        socket: Arc<UdpSocket>,
        wireguard: Arc<RwLock<WireGuardDevice>>,
        registry: Arc<DomainRegistry>,
    ) -> Self {
        let foca = Arc::new(Mutex::new(Self::init_foca(&local_node, None)));
        let (timer_tx, timer_rx) = mpsc::channel(256);
        let (leader_tx, leader_rx) = watch::channel(None);
        let (membership_tx, _) = tokio::sync::broadcast::channel(64);
        let (workload_tx, _) = tokio::sync::broadcast::channel(64);
        let election_state = Arc::new(RwLock::new(ElectionState::new()));
        Self {
            local_node,
            socket,
            peers: Arc::new(RwLock::new(HashMap::new())),
            wireguard,
            registry,
            foca,
            gossip_period: Duration::from_millis(500),
            timer_tx,
            timer_rx: Arc::new(Mutex::new(timer_rx)),
            election_state,
            leader_tx: Arc::new(leader_tx),
            leader_rx,
            membership_tx: Arc::new(membership_tx),
            workload_tx: Arc::new(workload_tx),
            election_timeout: Duration::from_millis(300),
            ack_timeout: Duration::from_millis(300),
            local_only_domains: Arc::new(RwLock::new(HashSet::new())),
            seeds: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Sets custom gossip period duration.
    pub fn with_gossip_period(mut self, period: Duration) -> Self {
        self.gossip_period = period;
        self
    }

    /// Sets custom Bully election timeout duration.
    pub fn with_election_timeout(mut self, timeout: Duration) -> Self {
        self.election_timeout = timeout;
        self
    }

    /// Sets custom Bully coordinator acknowledgment timeout duration.
    pub fn with_ack_timeout(mut self, timeout: Duration) -> Self {
        self.ack_timeout = timeout;
        self
    }

    /// Marks domains as node-local: they are excluded from route gossip so
    /// they never spread to peers (used for the leader-only dashboard ingress).
    pub async fn set_local_only_domains(&self, domains: HashSet<String>) {
        *self.local_only_domains.write().await = domains;
    }

    /// Creates a controller with custom Foca configuration (convenient for fast timeout testing).
    pub fn new_with_foca_config(
        local_node: PeerNode,
        socket: Arc<UdpSocket>,
        wireguard: Arc<RwLock<WireGuardDevice>>,
        registry: Arc<DomainRegistry>,
        config: FocaConfig,
    ) -> Self {
        let foca = Arc::new(Mutex::new(Self::init_foca(&local_node, Some(config))));
        let (timer_tx, timer_rx) = mpsc::channel(256);
        let (leader_tx, leader_rx) = watch::channel(None);
        let (membership_tx, _) = tokio::sync::broadcast::channel(64);
        let (workload_tx, _) = tokio::sync::broadcast::channel(64);
        let election_state = Arc::new(RwLock::new(ElectionState::new()));
        Self {
            local_node,
            socket,
            peers: Arc::new(RwLock::new(HashMap::new())),
            wireguard,
            registry,
            foca,
            gossip_period: Duration::from_millis(500),
            timer_tx,
            timer_rx: Arc::new(Mutex::new(timer_rx)),
            election_state,
            leader_tx: Arc::new(leader_tx),
            leader_rx,
            membership_tx: Arc::new(membership_tx),
            workload_tx: Arc::new(workload_tx),
            election_timeout: Duration::from_millis(300),
            ack_timeout: Duration::from_millis(300),
            local_only_domains: Arc::new(RwLock::new(HashSet::new())),
            seeds: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn init_foca(
        node: &PeerNode,
        config: Option<FocaConfig>,
    ) -> Foca<PeerNode, PostcardCodec, StdRng, NoCustomBroadcast> {
        let foca_config = config.unwrap_or_else(|| {
            let mut c = FocaConfig::simple();
            c.probe_period = Duration::from_millis(500);
            c.probe_rtt = Duration::from_millis(150);
            c.suspect_to_down_after = Duration::from_millis(1000);
            // Purge "Down" members quickly so restarted nodes (which start
            // with a fresh incarnation) can rejoin without waiting out
            // long stale-state timeouts.
            c.remove_down_after = Some(Duration::from_secs(2));
            c
        });
        let rng: StdRng = rand::make_rng();
        Foca::new(node.clone(), foca_config, rng, PostcardCodec)
    }

    /// Sends a `JoinRequest` to seed nodes and awaits a `JoinResponse` containing active peers.
    pub async fn bootstrap(&self, seeds: &[SocketAddr]) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        *self.seeds.write().await = seeds.to_vec();
        if seeds.is_empty() {
            return Ok(0);
        }

        self.send_join_requests().await?;

        // Wait for JoinResponse with timeout
        let mut buf = vec![0u8; 65535];
        let timeout = Duration::from_millis(1500);

        let res = tokio::time::timeout(timeout, async {
            loop {
                let (len, src) = self.socket.recv_from(&mut buf).await?;
                if let Ok(ClusterMessage::JoinResponse { peers }) = ClusterMessage::decode(&buf[..len]) {
                    tracing::info!(src = %src, peer_count = peers.len(), "received UDP JoinResponse");
                    return Ok::<Vec<PeerNode>, Box<dyn std::error::Error + Send + Sync>>(peers);
                }
            }
        }).await;

        match res {
            Ok(Ok(peers)) => {
                let count = peers.len();
                for peer in peers {
                    self.register_peer(peer).await;
                }
                // Announce our presence to all peers now that we know them
                self.broadcast_announce().await?;
                Ok(count)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                tracing::warn!("timeout awaiting JoinResponse from seeds; proceeding as independent node");
                Ok(0)
            }
        }
    }

    /// Sends `JoinRequest` datagrams to all configured seeds (fire-and-forget).
    /// Responses are handled by the main datagram loop.
    pub async fn send_join_requests(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let seeds = self.seeds.read().await.clone();
        let join_msg = ClusterMessage::JoinRequest {
            node: self.local_node.clone(),
        };
        let encoded = join_msg.encode()?;
        let mut sent = 0;
        for seed in seeds {
            if seed != self.local_node.endpoint {
                tracing::info!(seed = %seed, "sending UDP JoinRequest to seed node");
                let _ = self.socket.send_to(&encoded, seed).await;
                sent += 1;
            }
        }
        Ok(sent)
    }

    /// If this node is leader and a higher-ranked peer just appeared,
    /// restart the bully election so the correct node takes leadership.
    pub async fn maybe_challenge_leadership(&self) {
        let role = { self.election_state.read().await.role };
        if role != ElectionRole::Leader {
            return;
        }
        let my_rank = self.local_node.rank();
        let higher_exists = {
            let peers = self.peers.read().await;
            peers.values().any(|p| p.rank() > my_rank)
        };
        if higher_exists {
            tracing::info!("higher-priority peer joined; restarting bully election");
            let this = self.clone();
            tokio::spawn(async move {
                let _ = this.start_election().await;
            });
        }
    }

    /// Announces this node to all currently known peers via UDP datagram.
    pub async fn broadcast_announce(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let announce_msg = ClusterMessage::PeerAnnounce {
            peer: self.local_node.clone(),
        };
        let encoded = announce_msg.encode()?;

        let peers = self.peers.read().await;
        let mut count = 0;
        for peer in peers.values() {
            if peer.node_id != self.local_node.node_id {
                let _ = self.socket.send_to(&encoded, peer.endpoint).await;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Registers a peer in the local peer table, WireGuard device, DomainRegistry, and Foca SWIM engine.
    pub async fn register_peer(&self, peer: PeerNode) {
        if peer.node_id == self.local_node.node_id {
            return;
        }

        self.register_peer_internal(&peer).await;

        // Announce to Foca SWIM cluster engine
        let mut runtime = AccumulatingRuntime::new();
        {
            let mut foca_lock = self.foca.lock().await;
            let _ = foca_lock.announce(peer, &mut runtime);
        }
        self.dispatch_runtime(runtime).await;
    }

    async fn register_peer_internal(&self, peer: &PeerNode) {
        if peer.node_id == self.local_node.node_id {
            return;
        }

        tracing::info!(
            node_id = %peer.node_id,
            mesh_ip = %peer.mesh_ip,
            endpoint = %peer.endpoint,
            "registering peer node in cluster mesh"
        );

        // 1. Peer table
        let is_new = {
            let mut lock = self.peers.write().await;
            lock.insert(peer.node_id.clone(), peer.clone()).is_none()
        };
        if is_new {
            let _ = self.membership_tx.send(MemberEvent::Up(peer.clone()));
        }
        {
            let count = self.peers.read().await.len() + 1;
            let mut state = self.election_state.write().await;
            if state.total_known_nodes < count {
                state.total_known_nodes = count;
            }
        }

        // 2. WireGuard device
        {
            let mut wg = self.wireguard.write().await;
            let wg_peer = WireGuardPeer::new(
                peer.public_key.clone(),
                Some(peer.wireguard_endpoint()),
                vec![format!("{}/32", peer.mesh_ip)],
            );
            wg.add_peer(wg_peer);
            if let Err(err) = wg.sync_to_kernel() {
                tracing::warn!(%err, "failed to sync WireGuard peers to kernel");
            }
        }

        // 3. DomainRegistry
        {
            let mesh_socket = SocketAddr::new(peer.mesh_ip, 443);
            let node_record = Node::new(&peer.node_id, mesh_socket)
                .with_mesh_address(mesh_socket)
                .with_direct_address(peer.endpoint);
            self.registry.insert(
                format!("{}.node.internal", peer.node_id),
                Route::new(Some(mesh_socket), node_record),
            );
        }
    }

    /// Unregisters an evicted or departed peer.
    pub async fn unregister_peer(&self, node_id: &str) -> Option<PeerNode> {
        self.unregister_peer_internal(node_id).await
    }

    async fn unregister_peer_internal(&self, node_id: &str) -> Option<PeerNode> {
        let removed = {
            let mut lock = self.peers.write().await;
            lock.remove(node_id)
        };

        if let Some(peer) = &removed {
            tracing::info!(node_id = %node_id, "unregistering peer node from cluster mesh");
            {
                // Quorum shrinks with the confirmed-dead member removed.
                let mut state = self.election_state.write().await;
                let remaining = self.peers.read().await.len() + 1;
                if state.total_known_nodes > remaining {
                    state.total_known_nodes = remaining;
                }
            }
            let mut wg = self.wireguard.write().await;
            wg.remove_peer(&peer.public_key);
            if let Err(err) = wg.sync_to_kernel() {
                tracing::warn!(%err, "failed to sync WireGuard peers to kernel");
            }
            self.registry.remove(&format!("{}.node.internal", node_id));
        }

        removed
    }

    /// Returns the currently acknowledged cluster leader, if known.
    pub async fn current_leader(&self) -> Option<PeerNode> {
        self.election_state.read().await.current_leader.clone()
    }

    /// Returns true if this local node is currently the elected cluster leader.
    pub async fn is_leader(&self) -> bool {
        self.election_state.read().await.role == ElectionRole::Leader
    }

    /// Returns the current lifecycle role of this node in the election protocol.
    pub async fn election_role(&self) -> ElectionRole {
        self.election_state.read().await.role
    }

    /// Returns the current monotonic election term.
    pub async fn election_term(&self) -> u64 {
        self.election_state.read().await.term
    }

    /// Subscribes to leadership transitions via a Tokio watch receiver.
    pub fn leader_watch(&self) -> watch::Receiver<Option<PeerNode>> {
        self.leader_rx.clone()
    }

    /// Subscribes to cluster membership events (nodes joining or failing).
    pub fn subscribe_membership(&self) -> tokio::sync::broadcast::Receiver<MemberEvent> {
        self.membership_tx.subscribe()
    }

    /// Subscribes to workload failover commands dispatched across the cluster.
    pub fn subscribe_workload_commands(&self) -> tokio::sync::broadcast::Receiver<WorkloadCommand> {
        self.workload_tx.subscribe()
    }

    /// Dispatches a command instructing a target node to spawn a duplicated workload for a failed service.
    pub async fn send_spawn_workload(
        &self,
        target_node: &str,
        domain: String,
        image: String,
        env: Vec<String>,
        container_port: u16,
        origin_node: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = {
            let peers = self.peers.read().await;
            peers.get(target_node).map(|p| p.endpoint)
        };

        if let Some(target_endpoint) = endpoint {
            let msg = ClusterMessage::SpawnWorkload {
                from_node: self.local_node.node_id.clone(),
                domain,
                image,
                env,
                container_port,
                origin_node,
            };
            let encoded = msg.encode()?;
            self.socket.send_to(&encoded, target_endpoint).await?;
        }
        Ok(())
    }

    /// Dispatches a command instructing a target node to stop a duplicated workload after failback.
    pub async fn send_stop_workload(
        &self,
        target_node: &str,
        domain: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = {
            let peers = self.peers.read().await;
            peers.get(target_node).map(|p| p.endpoint)
        };

        if let Some(target_endpoint) = endpoint {
            let msg = ClusterMessage::StopWorkload {
                from_node: self.local_node.node_id.clone(),
                domain,
            };
            let encoded = msg.encode()?;
            self.socket.send_to(&encoded, target_endpoint).await?;
        }
        Ok(())
    }

    /// Dispatches a confirmation acknowledgment when a workload was successfully spawned.
    pub async fn send_workload_spawned(
        &self,
        target_node: &str,
        domain: String,
        target_addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = {
            let peers = self.peers.read().await;
            peers.get(target_node).map(|p| p.endpoint)
        };

        if let Some(target_endpoint) = endpoint {
            let msg = ClusterMessage::WorkloadSpawned {
                from_node: self.local_node.node_id.clone(),
                domain,
                target_addr,
            };
            let encoded = msg.encode()?;
            self.socket.send_to(&encoded, target_endpoint).await?;
        }
        Ok(())
    }

    /// Explicitly configures required quorum size (overriding dynamic majority calculation).
    pub async fn set_quorum_size(&self, size: Option<usize>) {
        self.election_state.write().await.quorum_size = size;
    }

    /// Manually sets total known nodes count for quorum majority calculation.
    pub async fn set_total_known_nodes(&self, total: usize) {
        self.election_state.write().await.total_known_nodes = total;
    }

    /// Starts a Bully election round.
    /// If higher-ranking peers exist, sends `Election` to them and awaits `ElectionOk`.
    /// If no higher-ranking peers exist or none reply within timeout, declares `Coordinator` and verifies quorum.
    pub async fn start_election(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let (term, higher_peers) = {
            let mut state = self.election_state.write().await;
            state.term += 1;
            state.role = ElectionRole::Candidate;
            state.acks.clear();
            state.got_election_ok = false;

            let peers = self.peers.read().await;
            if state.total_known_nodes <= 1 || state.total_known_nodes < peers.len() + 1 {
                state.total_known_nodes = peers.len() + 1;
            }

            let higher: Vec<PeerNode> = peers
                .values()
                .filter(|p| p.rank() > self.local_node.rank())
                .cloned()
                .collect();

            (state.term, higher)
        };

        tracing::info!(
            node_id = %self.local_node.node_id,
            term,
            higher_count = higher_peers.len(),
            "starting bully election"
        );

        if higher_peers.is_empty() {
            // Local node is the highest ranked among known peers!
            return self.declare_coordinator(term).await;
        }

        // Broadcast Election message to higher peers
        let election_msg = ClusterMessage::Election {
            from_node: self.local_node.node_id.clone(),
            term,
            priority: self.local_node.priority,
        };
        let encoded = election_msg.encode()?;
        for higher in &higher_peers {
            let _ = self.socket.send_to(&encoded, higher.endpoint).await;
        }

        // Await ElectionOk with timeout
        tokio::time::sleep(self.election_timeout).await;

        let got_ok = {
            let state = self.election_state.read().await;
            if state.term != term {
                return Ok(false);
            }
            state.got_election_ok
        };

        if got_ok {
            tracing::debug!(term, "higher peer answered ElectionOk; waiting for coordinator declaration");
            // Wait for higher peer to broadcast Coordinator
            tokio::time::sleep(self.election_timeout * 2).await;
            let state = self.election_state.read().await;
            if state.current_leader.is_some() {
                return Ok(false);
            }
            // Higher peer failed to declare coordinator within timeout; restart election
            drop(state);
            tracing::warn!(term, "higher peer failed to declare coordinator within timeout; restarting election");
            return Box::pin(self.start_election()).await;
        } else {
            // Higher peers unresponsive or dead; local node steps up!
            tracing::info!(term, "no higher peers answered; declaring coordinator");
            return self.declare_coordinator(term).await;
        }
    }

    /// Declares local node as cluster coordinator and broadcasts `Coordinator` to all peers,
    /// verifying quorum before assuming the Leader role.
    pub async fn declare_coordinator(&self, term: u64) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let (coordinator_msg, peer_endpoints, q_req) = {
            let mut state = self.election_state.write().await;
            if state.term != term {
                return Ok(false);
            }
            state.role = ElectionRole::Candidate;
            state.acks.clear();

            let peers = self.peers.read().await;
            if state.total_known_nodes <= 1 || state.total_known_nodes < peers.len() + 1 {
                state.total_known_nodes = peers.len() + 1;
            }
            let q = state.quorum_required();

            let msg = ClusterMessage::Coordinator {
                leader: self.local_node.clone(),
                term,
            };
            let endpoints: Vec<SocketAddr> = peers.values().map(|p| p.endpoint).collect();
            (msg, endpoints, q)
        };

        let encoded = coordinator_msg.encode()?;
        for ep in &peer_endpoints {
            let _ = self.socket.send_to(&encoded, *ep).await;
        }

        // Single-node cluster reaches quorum trivially
        if q_req <= 1 {
            let mut state = self.election_state.write().await;
            if state.term == term {
                state.role = ElectionRole::Leader;
                state.current_leader = Some(self.local_node.clone());
                let _ = self.leader_tx.send(Some(self.local_node.clone()));
                tracing::info!(node_id = %self.local_node.node_id, term, "elected as cluster coordinator (single-node quorum)");
                return Ok(true);
            }
        }

        // Wait for ACKs to accumulate up to ack_timeout
        let start = tokio::time::Instant::now();
        while start.elapsed() < self.ack_timeout {
            {
                let state = self.election_state.read().await;
                if state.term != term {
                    return Ok(false);
                }
                if state.role == ElectionRole::Leader {
                    return Ok(true);
                }
                if state.has_quorum() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let mut state = self.election_state.write().await;
        if state.term != term {
            return Ok(false);
        }
        if state.has_quorum() {
            state.role = ElectionRole::Leader;
            state.current_leader = Some(self.local_node.clone());
            let _ = self.leader_tx.send(Some(self.local_node.clone()));
            tracing::info!(
                node_id = %self.local_node.node_id,
                term,
                acks = state.acks.len(),
                quorum = state.quorum_required(),
                "elected as cluster coordinator (quorum reached)"
            );
            Ok(true)
        } else {
            tracing::warn!(
                node_id = %self.local_node.node_id,
                term,
                votes = state.acks.len() + 1,
                required = state.quorum_required(),
                "coordinator declaration failed quorum check"
            );
            state.role = ElectionRole::Follower;
            Ok(false)
        }
    }

    /// Administratively force-specifies a cluster leader.
    ///
    /// If `target_node_id` is the local node, it assumes leadership immediately,
    /// increments the election term, and broadcasts a `Coordinator` announcement to all peers.
    ///
    /// If `target_node_id` is a known peer, this node designates that peer as leader,
    /// increments its term, and broadcasts a `Coordinator` announcement across the fleet.
    pub async fn force_leader(&self, target_node_id: &str) -> Result<(PeerNode, u64), Box<dyn std::error::Error + Send + Sync>> {
        let (leader_node, new_term, peer_endpoints) = {
            let mut state = self.election_state.write().await;
            let term = state.term + 1;
            state.term = term;

            let peers = self.peers.read().await;
            let target = if target_node_id == self.local_node.node_id {
                state.role = ElectionRole::Leader;
                state.current_leader = Some(self.local_node.clone());
                let _ = self.leader_tx.send(Some(self.local_node.clone()));
                self.local_node.clone()
            } else if let Some(peer) = peers.get(target_node_id) {
                state.role = ElectionRole::Follower;
                state.current_leader = Some(peer.clone());
                let _ = self.leader_tx.send(Some(peer.clone()));
                peer.clone()
            } else {
                return Err(format!("Target node '{target_node_id}' not found in cluster peers").into());
            };

            let endpoints: Vec<SocketAddr> = peers.values().map(|p| p.endpoint).collect();
            (target, term, endpoints)
        };

        // Broadcast Coordinator declaration across the fleet
        let msg = ClusterMessage::Coordinator {
            leader: leader_node.clone(),
            term: new_term,
        };
        let encoded = msg.encode()?;
        for ep in &peer_endpoints {
            let _ = self.socket.send_to(&encoded, *ep).await;
        }

        tracing::warn!(
            forced_leader = %leader_node.node_id,
            term = new_term,
            "administratively forced cluster leader"
        );
        Ok((leader_node, new_term))
    }

    /// Administratively steps down from leadership, triggering an election among remaining nodes.
    pub async fn step_down(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let is_leader = self.is_leader().await;
        if !is_leader {
            return Ok(false);
        }

        let (new_term, endpoints) = {
            let mut state = self.election_state.write().await;
            state.term += 1;
            state.role = ElectionRole::Follower;
            state.current_leader = None;
            let _ = self.leader_tx.send(None);
            let peers = self.peers.read().await;
            let eps: Vec<SocketAddr> = peers.values().map(|p| p.endpoint).collect();
            (state.term, eps)
        };

        tracing::info!(term = new_term, "local leader stepped down; notifying peers to elect new leader");
        let election_msg = ClusterMessage::Election {
            from_node: self.local_node.node_id.clone(),
            term: new_term,
            priority: 0,
        };
        if let Ok(encoded) = election_msg.encode() {
            for ep in endpoints {
                let _ = self.socket.send_to(&encoded, ep).await;
            }
        }

        Ok(true)
    }

    /// Administratively drops a node from the cluster and mesh.
    ///
    /// Evicts the peer from known cluster peers, unprograms its WireGuard peer interface,
    /// removes associated routes from the registry, and emits `MemberEvent::Down`.
    pub async fn drop_node(&self, target_node_id: &str) -> Result<PeerNode, Box<dyn std::error::Error + Send + Sync>> {
        if target_node_id == self.local_node.node_id {
            return Err("Cannot drop local node from itself; stop the Bridge daemon service instead".into());
        }

        let removed = self.unregister_peer_internal(target_node_id).await;
        match removed {
            Some(peer) => {
                let _ = self.membership_tx.send(MemberEvent::Down(peer.clone()));

                // Remove any routes whose target was this node
                let snapshot = self.registry.snapshot();
                for (domain, route) in snapshot.iter() {
                    if route.node.node_id == target_node_id {
                        self.registry.remove(domain);
                        tracing::info!(domain = %domain, dropped_node = %target_node_id, "pruned route for dropped node");
                    }
                }

                // If the dropped node was the current leader, trigger immediate election
                let was_leader = {
                    let mut state = self.election_state.write().await;
                    if let Some(leader) = &state.current_leader {
                        if leader.node_id == target_node_id {
                            state.current_leader = None;
                            let _ = self.leader_tx.send(None);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if was_leader {
                    tracing::info!(dropped_leader = %target_node_id, "dropped leader node; triggering election");
                    let this = self.clone();
                    tokio::spawn(async move {
                        let _ = this.start_election().await;
                    });
                }

                Ok(peer)
            }
            None => Err(format!("Node '{target_node_id}' not found in cluster peers").into()),
        }
    }

    /// Forces a synchronization of the in-memory WireGuard peers table to the Linux kernel interface.
    pub async fn sync_wireguard_kernel(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let wg = self.wireguard.write().await;
        wg.sync_to_kernel()?;
        Ok(wg.peer_count())
    }

    /// Registers or updates a local route and immediately disseminates it across the cluster.
    pub async fn broadcast_route(
        &self,
        domain: String,
        mut route: Route,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let next_ver = match self.registry.lookup_raw(&domain) {
            Some(existing) => existing.clock.map(|c| c.version + 1).unwrap_or(1),
            None => 1,
        };
        route.clock = Some(registry::RouteClock::new(&self.local_node.node_id, next_ver));

        // Insert into local registry
        self.registry.insert(domain.clone(), route.clone());

        // Broadcast RouteSync to all known peers
        let mut routes = HashMap::new();
        routes.insert(domain, route);
        let sync_msg = ClusterMessage::RouteSync {
            from_node: self.local_node.node_id.clone(),
            routes,
        };
        let encoded = sync_msg.encode()?;

        let peers = self.peers.read().await;
        for peer in peers.values() {
            if peer.node_id != self.local_node.node_id {
                let _ = self.socket.send_to(&encoded, peer.endpoint).await;
            }
        }
        Ok(())
    }

    /// Performs one round of periodic push-pull routing table digest gossip to `fanout` peers.
    pub async fn gossip_routes(
        &self,
        fanout: usize,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let targets: Vec<SocketAddr> = {
            let peers = self.peers.read().await;
            if peers.is_empty() {
                return Ok(0);
            }
            peers
                .values()
                .filter(|p| p.node_id != self.local_node.node_id)
                .take(fanout)
                .map(|p| p.endpoint)
                .collect()
        };

        if targets.is_empty() {
            return Ok(0);
        }

        let mut digest = self.registry.digest();
        {
            let excluded = self.local_only_domains.read().await;
            digest.retain(|domain, _| !excluded.contains(domain));
        }
        let digest_msg = ClusterMessage::RouteDigest {
            from_node: self.local_node.node_id.clone(),
            digest,
        };
        let encoded = digest_msg.encode()?;

        let mut sent = 0;
        for endpoint in targets {
            let _ = self.socket.send_to(&encoded, endpoint).await;
            sent += 1;
        }

        Ok(sent)
    }

    /// Drains and executes pending network transmissions, timers, and notifications emitted by Foca.
    pub async fn dispatch_runtime(&self, mut runtime: AccumulatingRuntime<PeerNode>) {
        while let Some((dst, packet)) = runtime.to_send() {
            let swim_msg = ClusterMessage::SwimPayload(packet.to_vec());
            if let Ok(encoded) = swim_msg.encode() {
                let _ = self.socket.send_to(&encoded, dst.endpoint).await;
            }
        }

        while let Some((after, timer)) = runtime.to_schedule() {
            let tx = self.timer_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(after).await;
                let _ = tx.send(timer).await;
            });
        }

        while let Some(notification) = runtime.to_notify() {
            match notification {
                OwnedNotification::MemberUp(node) => {
                    tracing::info!(node = %node.node_id, endpoint = %node.endpoint, "foca SWIM detected member UP");
                    self.register_peer_internal(&node).await;
                    let _ = self.membership_tx.send(MemberEvent::Up(node));
                }
                OwnedNotification::MemberDown(node) => {
                    tracing::warn!(node = %node.node_id, endpoint = %node.endpoint, "foca SWIM detected member DOWN (failure detected)");
                    self.unregister_peer_internal(&node.node_id).await;
                    let _ = self.membership_tx.send(MemberEvent::Down(node.clone()));

                    let was_leader = {
                        let mut state = self.election_state.write().await;
                        if let Some(leader) = &state.current_leader {
                            if leader.node_id == node.node_id {
                                state.current_leader = None;
                                let _ = self.leader_tx.send(None);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if was_leader {
                        tracing::info!(dead_leader = %node.node_id, "cluster leader failed; initiating bully election");
                        let this = self.clone();
                        tokio::spawn(async move {
                            let _ = this.start_election().await;
                        });
                    }
                }
                _ => {}
            }
        }
    }

    /// Processes an incoming datagram from `src`.
    pub async fn handle_datagram(
        &self,
        src: SocketAddr,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let msg = ClusterMessage::decode(data)?;

        match msg {
            ClusterMessage::JoinRequest { node } => {
                tracing::info!(from = %node.node_id, src = %src, "handling incoming JoinRequest");
                // 1. Register the joining node
                self.register_peer(node.clone()).await;
                self.maybe_challenge_leadership().await;

                // 2. Collect current peers list to send back
                let mut peer_list = Vec::new();
                peer_list.push(self.local_node.clone());
                {
                    let lock = self.peers.read().await;
                    for p in lock.values() {
                        if p.node_id != node.node_id {
                            peer_list.push(p.clone());
                        }
                    }
                }

                // 3. Respond with JoinResponse
                let response = ClusterMessage::JoinResponse { peers: peer_list };
                let encoded = response.encode()?;

                // 4. Announce this new peer to all other nodes in the background
                let announce = ClusterMessage::PeerAnnounce { peer: node };
                if let Ok(announce_bytes) = announce.encode() {
                    let lock = self.peers.read().await;
                    for p in lock.values() {
                        if p.endpoint != src && p.node_id != self.local_node.node_id {
                            let _ = self.socket.send_to(&announce_bytes, p.endpoint).await;
                        }
                    }
                }

                Ok(Some(encoded))
            }
            ClusterMessage::JoinResponse { peers } => {
                for peer in peers {
                    self.register_peer(peer).await;
                }
                self.maybe_challenge_leadership().await;
                Ok(None)
            }
            ClusterMessage::PeerAnnounce { peer } => {
                self.register_peer(peer).await;
                self.maybe_challenge_leadership().await;
                Ok(None)
            }
            ClusterMessage::Ping { seq, from_node: _ } => {
                let ack = ClusterMessage::Ack {
                    seq,
                    from_node: self.local_node.node_id.clone(),
                };
                Ok(Some(ack.encode()?))
            }
            ClusterMessage::Ack { seq, from_node } => {
                tracing::debug!(seq, from = %from_node, "received SWIM ping ack");
                Ok(None)
            }
            ClusterMessage::PingReq { seq, target_node, target_endpoint } => {
                // Forward ping to indirect target
                let ping = ClusterMessage::Ping {
                    seq,
                    from_node: format!("{}:via:{}", self.local_node.node_id, target_node),
                };
                let _ = self.socket.send_to(&ping.encode()?, target_endpoint).await;
                Ok(None)
            }
            ClusterMessage::SwimPayload(payload) => {
                let mut runtime = AccumulatingRuntime::new();
                {
                    let mut foca_lock = self.foca.lock().await;
                    let _ = foca_lock.handle_data(&payload, &mut runtime);
                }
                self.dispatch_runtime(runtime).await;
                Ok(None)
            }
            ClusterMessage::RouteDigest { from_node: _, digest } => {
                let (needed, push) = self.registry.compare_digest(&digest);

                // 1. If we have routes the remote peer needs, push them
                if !push.is_empty() {
                    let sync_msg = ClusterMessage::RouteSync {
                        from_node: self.local_node.node_id.clone(),
                        routes: push,
                    };
                    let _ = self.socket.send_to(&sync_msg.encode()?, src).await;
                }

                // 2. If the remote peer has routes we need, pull them
                if !needed.is_empty() {
                    let pull_msg = ClusterMessage::RoutePull {
                        from_node: self.local_node.node_id.clone(),
                        domains: needed,
                    };
                    let _ = self.socket.send_to(&pull_msg.encode()?, src).await;
                }

                Ok(None)
            }
            ClusterMessage::RoutePull { from_node: _, domains } => {
                let excluded = self.local_only_domains.read().await;
                let mut requested_routes = HashMap::new();
                for domain in domains {
                    if excluded.contains(&domain) {
                        continue;
                    }
                    if let Some(route) = self.registry.lookup_raw(&domain) {
                        requested_routes.insert(domain, route);
                    }
                }

                if !requested_routes.is_empty() {
                    let sync_msg = ClusterMessage::RouteSync {
                        from_node: self.local_node.node_id.clone(),
                        routes: requested_routes,
                    };
                    let _ = self.socket.send_to(&sync_msg.encode()?, src).await;
                }

                Ok(None)
            }
            ClusterMessage::RouteSync { from_node, routes } => {
                tracing::info!(from = %from_node, count = routes.len(), "received RouteSync gossip");
                let excluded = self.local_only_domains.read().await;
                for (domain, route) in routes {
                    if excluded.contains(&domain) {
                        continue;
                    }
                    self.registry.insert_versioned(domain, route);
                }
                Ok(None)
            }
            ClusterMessage::Election { from_node, term, priority } => {
                let sender_rank = (priority, from_node.as_str());
                let my_rank = self.local_node.rank();

                if my_rank > sender_rank {
                    let ok_msg = ClusterMessage::ElectionOk {
                        from_node: self.local_node.node_id.clone(),
                        term,
                    };
                    let _ = self.socket.send_to(&ok_msg.encode()?, src).await;

                    let role = { self.election_state.read().await.role };
                    if role == ElectionRole::Leader {
                        // Adopt the higher incoming term so our Coordinator
                        // declaration is never rejected as stale by the peer.
                        let current_term = {
                            let mut state = self.election_state.write().await;
                            if term > state.term {
                                state.term = term;
                            }
                            state.term
                        };
                        let coord = ClusterMessage::Coordinator {
                            leader: self.local_node.clone(),
                            term: current_term,
                        };
                        let _ = self.socket.send_to(&coord.encode()?, src).await;
                    } else {
                        let this = self.clone();
                        tokio::spawn(async move {
                            let _ = this.start_election().await;
                        });
                    }
                }
                Ok(None)
            }
            ClusterMessage::ElectionOk { from_node, term } => {
                let mut state = self.election_state.write().await;
                if state.term == term {
                    state.got_election_ok = true;
                    tracing::debug!(from = %from_node, term, "received ElectionOk from higher peer");
                }
                Ok(None)
            }
            ClusterMessage::Coordinator { leader, term } => {
                let should_ack = {
                    let mut state = self.election_state.write().await;
                    if term >= state.term {
                        state.term = term;
                        state.role = if leader.node_id == self.local_node.node_id {
                            ElectionRole::Leader
                        } else {
                            ElectionRole::Follower
                        };
                        state.current_leader = Some(leader.clone());
                        let _ = self.leader_tx.send(Some(leader.clone()));
                        true
                    } else {
                        false
                    }
                };

                if should_ack {
                    tracing::info!(leader = %leader.node_id, term, "acknowledged cluster coordinator");
                    let ack_msg = ClusterMessage::CoordinatorAck {
                        from_node: self.local_node.node_id.clone(),
                        term,
                    };
                    Ok(Some(ack_msg.encode()?))
                } else {
                    Ok(None)
                }
            }
            ClusterMessage::CoordinatorAck { from_node, term } => {
                let mut state = self.election_state.write().await;
                if state.term == term && state.role == ElectionRole::Candidate {
                    state.acks.insert(from_node);
                    if state.has_quorum() {
                        state.role = ElectionRole::Leader;
                        state.current_leader = Some(self.local_node.clone());
                        let _ = self.leader_tx.send(Some(self.local_node.clone()));
                        tracing::info!(node_id = %self.local_node.node_id, term, "quorum reached via CoordinatorAck; promoted to Leader");
                    }
                }
                Ok(None)
            }
            ClusterMessage::SpawnWorkload {
                from_node,
                domain,
                image,
                env,
                container_port,
                origin_node,
            } => {
                let _ = self.workload_tx.send(WorkloadCommand::Spawn {
                    from_node,
                    domain,
                    image,
                    env,
                    container_port,
                    origin_node,
                });
                Ok(None)
            }
            ClusterMessage::StopWorkload { from_node, domain } => {
                let _ = self.workload_tx.send(WorkloadCommand::Stop { from_node, domain });
                Ok(None)
            }
            ClusterMessage::WorkloadSpawned {
                from_node,
                domain,
                target_addr,
            } => {
                let _ = self.workload_tx.send(WorkloadCommand::Spawned {
                    from_node,
                    domain,
                    target_addr,
                });
                Ok(None)
            }
        }
    }

    /// Runs the UDP cluster packet loop, SWIM failure detection, and periodic route table gossip until shutdown.
    pub async fn run_with_shutdown(
        self: Arc<Self>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<proxy::ShutdownReason>,
        ack_tx: Option<tokio::sync::mpsc::Sender<proxy::ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buf = vec![0u8; 65535];
        tracing::info!(
            endpoint = %self.local_node.endpoint,
            "raw UDP cluster controller listening with SWIM failure detection and route gossip"
        );

        let mut timer_rx = self.timer_rx.lock().await;
        let mut gossip_ticker = tokio::time::interval(self.gossip_period);
        let mut rejoin_ticker = tokio::time::interval(Duration::from_secs(3));
        let mut leader_check_ticker = tokio::time::interval(Duration::from_secs(5));

        {
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if this.current_leader().await.is_none() {
                    tracing::info!("no cluster leader after startup grace period; initiating bully election");
                    let _ = this.start_election().await;
                }
            });
        }

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::info!("cluster controller received shutdown signal; broadcasting SWIM leave");
                    let mut runtime = AccumulatingRuntime::new();
                    {
                        let mut foca_lock = self.foca.lock().await;
                        let _ = foca_lock.leave_cluster(&mut runtime);
                    }
                    self.dispatch_runtime(runtime).await;
                    break;
                }
                _ = gossip_ticker.tick() => {
                    let _ = self.gossip_routes(3).await;
                }
                _ = rejoin_ticker.tick() => {
                    let peers = self.peers.read().await;
                    if peers.is_empty() {
                        drop(peers);
                        // No peers yet (e.g. simultaneous fleet boot): retry seeds
                        // so nodes that missed each other's first JoinRequest converge.
                        let _ = self.send_join_requests().await;
                    } else {
                        // Refresh SWIM membership for all known peers: if foca
                        // purged one (e.g. stale Down state), this re-adds and
                        // resumes probing it.
                        let mut runtime = AccumulatingRuntime::new();
                        {
                            let mut foca_lock = self.foca.lock().await;
                            for peer in peers.values() {
                                let _ = foca_lock.announce(peer.clone(), &mut runtime);
                            }
                        }
                        self.dispatch_runtime(runtime).await;
                    }
                }
                _ = leader_check_ticker.tick() => {
                    // Self-healing: if we know peers but have no acknowledged leader
                    // (e.g. an election failed quorum during a transient flap), rerun
                    // the bully election until the cluster converges.
                    let peers_empty = self.peers.read().await.is_empty();
                    let has_leader = self.current_leader().await.is_some();
                    if !peers_empty && !has_leader {
                        tracing::info!("peers known but no acknowledged leader; rerunning bully election");
                        let this = self.clone();
                        tokio::spawn(async move {
                            let _ = this.start_election().await;
                        });
                    }
                }
                Some(timer_event) = timer_rx.recv() => {
                    let mut runtime = AccumulatingRuntime::new();
                    {
                        let mut foca_lock = self.foca.lock().await;
                        let _ = foca_lock.handle_timer(timer_event, &mut runtime);
                    }
                    self.dispatch_runtime(runtime).await;
                }
                recv_res = self.socket.recv_from(&mut buf) => {
                    match recv_res {
                        Ok((len, src)) => {
                            let data = &buf[..len];
                            match self.handle_datagram(src, data).await {
                                Ok(Some(resp)) => {
                                    let _ = self.socket.send_to(&resp, src).await;
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    tracing::debug!(src = %src, %err, "invalid cluster packet received");
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(%err, "error receiving from cluster UDP socket");
                        }
                    }
                }
            }
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(proxy::ShutdownAck {
                    subsystem: "cluster_mesh".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}
