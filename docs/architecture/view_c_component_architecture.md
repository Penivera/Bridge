# View C: Internal Component Architecture

## 1. Crate Architecture & Dependency Direction

The repository is structured as a Cargo workspace with four member libraries and one root binary crate:

```mermaid
graph TD
    %% Workspace Crates
    BridgeCrate["bridge (Root Binary & Daemon Orchestrator)\nsrc/main.rs • src/daemon.rs • src/cli.rs"]
    ProxyCrate["proxy (Ingress & Transport Engine)\nproxy/src/lib.rs"]
    ClusterCrate["cluster (SWIM Gossip & Consensus)\ncluster/src/lib.rs"]
    MeshCrate["mesh (WireGuard Device & Keys)\nmesh/src/lib.rs"]
    RegistryCrate["registry (Domain Routing & Hash Ring)\nregistry/src/lib.rs"]

    %% Dependencies
    BridgeCrate --> ProxyCrate
    BridgeCrate --> ClusterCrate
    BridgeCrate --> MeshCrate
    BridgeCrate --> RegistryCrate

    ProxyCrate --> RegistryCrate
    ClusterCrate --> RegistryCrate
    ClusterCrate --> MeshCrate

    %% Sub-dependencies
    ClusterCrate -.->|"Emits MemberEvent to"| BridgeCrate
    ClusterCrate -.->|"Shares ClusterController with"| BridgeCrate
```

* **`registry`:** Core data model. Zero internal workspace dependencies. Defines `DomainRegistry`, `Route`, `Node`, `HashRing`, and vector clocks.
* **`mesh`:** Manages WireGuard encryption key generation, peer definitions, and Linux kernel Netlink synchronization via `defguard_wireguard_rs`.
* **`cluster`:** Implements the binary cluster wire protocol (`BRDG`), Foca SWIM failure detection, Bully leader election with dynamic quorum, and push-pull route gossip.
* **`proxy`:** High-performance ingress engine. Implements Direct L7 proxying, Handoff L4 SNI passthrough, Managed Traefik dynamic provider, PROXY protocol v2, UDP/QUIC proxying, and the adaptive elastic worker pool.
* **`bridge` (root):** Application bootstrap orchestrator (`daemon.rs`), administrative CLI (`cli.rs`), IPC control server (`src/ipc`), embedded dashboard (`src/dashboard`), Docker discovery (`src/discovery`), failover duplicator (`src/failover`), ingress handoff managers (`src/handoff`), and self-updater (`src/updater.rs`).

---

## 2. Component Decomposition by Subsystem

### 2.1 Daemon Orchestrator (`bridge`)

```mermaid
classDiagram
    class Daemon {
        +run(config_path: Option<&Path>) Result
        +build_initial_routes(config, node_map) HashMap
        +configure_proxy_and_udp(config, node_map) ProxyConfig
        +apply_dashboard_ownership(registry, public_domain, local_node_id, leader_id, addr)
    }

    class Config {
        +enable_telemetry: bool
        +proxy: ProxyConfig
        +nodes: Vec<Node>
        +services: Vec<Service>
        +udp_services: Vec<UdpServiceConfig>
        +discovery: DiscoveryConfig
        +ipc: IpcConfig
        +dashboard: DashboardConfig
        +auth: AuthConfig
        +node: Option<NodeConfig>
        +seeds: Vec<SeedConfig>
        +handoff: HandoffConfig
        +load_auto(cli_override) Result<Config>
        +persist_peer_to_config_file(path, peer) Result<bool>
    }

    class ShutdownCoordinator {
        -broadcast_tx: broadcast::Sender
        -ack_tx: mpsc::Sender
        -ack_rx: mpsc::Receiver
        -expected_subsystems: HashSet<String>
        +register_subsystem(name) (Receiver, Sender)
        +broadcast_and_wait(reason, timeout) Vec<ShutdownAck>
        +wait_for_signal() ShutdownReason
    }

    Daemon --> Config : Loads
    Daemon --> ShutdownCoordinator : Registers Subsystems
```

### 2.2 Ingress Proxy Engine (`proxy`)

```mermaid
classDiagram
    class Proxy {
        +config: Arc<ProxyConfig>
        +registry: Arc<DomainRegistry>
        +client: Client
        +run() Result
        +run_with_coordinator(coordinator) Result
    }

    class ElasticWorkerPool {
        -injector: Injector<ConnectionTask>
        -stealers: RwLock<Vec<Stealer>>
        -monitor: Mutex<QueueDwellMonitor>
        -active_workers: AtomicUsize
        -max_concurrency: usize
        +dispatch(stream, client_addr)
        +try_scale_up() bool
    }

    class QueueDwellMonitor {
        -window: VecDeque<Duration>
        +record_dwell(duration)
        +average_dwell() Duration
    }

    class HandoffHandler {
        +handle_handoff_connection(client_stream, client_addr, registry, default_routing, redirect_http, send_proxy_protocol) Result
    }

    class SniParser {
        +parse_tls_sni(buf) Option<String>
        +parse_http_host(buf) Option<String>
        +parse_http_uri_path(buf) String
    }

    class ProxyV2Encoder {
        +encode_proxy_v2(src, dst) Vec<u8>
        +parse_proxy_v2(buf) Result<ProxyV2Header>
    }

    class ManagedProvider {
        +generate_traefik_config(registry, routing_pref, config) TraefikConfig
        +sync_traefik_file(path, content) Result<bool>
        +run_managed_provider_with_shutdown(...) Result
    }

    class UdpProxy {
        +run_udp_services_with_shutdown(services, shutdown_rx, ack_tx) Result
        +spawn_session(client_addr, upstream, timeout, ingress, sessions) Result
    }

    class QuicRouter {
        +parse_header(payload) Option<QuicHeaderSummary>
        +route_dcid(dcid) Option<SocketAddr>
    }

    Proxy --> ElasticWorkerPool : Dispatches TCP
    ElasticWorkerPool --> QueueDwellMonitor : Samples Latency
    ElasticWorkerPool --> HandoffHandler : In Handoff Mode
    HandoffHandler --> SniParser : Peeks SNI / Host
    HandoffHandler --> ProxyV2Encoder : Prepends PROXY v2 Header
    Proxy --> ManagedProvider : In Managed Mode
    Proxy --> UdpProxy : If UDP Services Configured
    UdpProxy --> QuicRouter : Optional QUIC Routing
```

### 2.3 Cluster Coordination & Consensus (`cluster`)

```mermaid
classDiagram
    class ClusterController {
        +local_node: PeerNode
        +socket: Arc<UdpSocket>
        +peers: Arc<RwLock<HashMap>>
        +wireguard: Arc<RwLock<WireGuardDevice>>
        +registry: Arc<DomainRegistry>
        +foca: Arc<Mutex<Foca>>
        +election_state: Arc<RwLock<ElectionState>>
        +bind(node, wg, registry) Result<ClusterController>
        +bootstrap(seeds) Result<usize>
        +start_election() Result<bool>
        +declare_coordinator(term) Result<bool>
        +gossip_routes(fanout) Result<usize>
        +handle_datagram(src, data) Result
        +run_with_shutdown(...) Result
    }

    class PeerNode {
        +node_id: String
        +public_key: String
        +mesh_ip: IpAddr
        +endpoint: SocketAddr
        +priority: u32
        +listen_port: u16
        +incarnation: u64
        +rank() (u32, &str)
    }

    class ElectionState {
        +current_leader: Option<PeerNode>
        +term: u64
        +role: ElectionRole
        +acks: HashSet<String>
        +total_known_nodes: usize
        +quorum_required() usize
        +has_quorum() bool
    }

    class ClusterMessage {
        <<enumeration>>
        JoinRequest
        JoinResponse
        PeerAnnounce
        Ping / Ack / PingReq
        SwimPayload
        RouteDigest
        RoutePull
        RouteSync
        Election / ElectionOk
        Coordinator / CoordinatorAck
        SpawnWorkload / StopWorkload / WorkloadSpawned
        +encode() Result<Vec<u8>>
        +decode(buf) Result<ClusterMessage>
    }

    ClusterController --> PeerNode : Tracks
    ClusterController --> ElectionState : Maintains Consensus
    ClusterController --> ClusterMessage : Serializes / Deserializes
```

### 2.4 Domain Registry & Consistent Hash Ring (`registry`)

```mermaid
classDiagram
    class DomainRegistry {
        -routes: ArcSwap<HashMap<String, Route>>
        -metrics: Mutex<HashMap<String, Arc<RouteMetrics>>>
        +lookup(domain) Option<Route>
        +insert(domain, route)
        +insert_versioned(domain, route) bool
        +remove(domain) Option<Route>
        +digest() HashMap<String, RouteClock>
        +compare_digest(remote) (Vec<String>, HashMap)
        +snapshot() Arc<HashMap>
        +record_request(host, latency_ms, is_error)
    }

    class Route {
        +upstream: Option<SocketAddr>
        +node: Node
        +targets: Vec<Node>
        +ring: Option<HashRing<Node>>
        +clock: Option<RouteClock>
        +is_tombstone: bool
        +select_node_for_client(client_ip, host) &Node
        +target_addr_for_client(client_ip, host, fallback) SocketAddr
    }

    class HashRing {
        -vnodes_per_node: usize (360)
        -ring: Vec<VirtualNode<T>>
        +add_node(node)
        +remove_node(node_id)
        +get_for_client(client_ip, host) Option<&T>
        +get_by_key(token) Option<&T>
    }

    class RouteClock {
        +origin_node: String
        +version: u64
        +cmp(other) Ordering
    }

    DomainRegistry --> Route : Stores Map
    Route --> HashRing : Balances Multi-Targets
    Route --> RouteClock : Lamport Vector Clock
```

### 2.5 Workload Duplication & Failover (`src/failover`)

```mermaid
classDiagram
    class WorkloadDuplicator {
        -local_node_id: String
        -configured_services: Vec<Service>
        -registry: Arc<DomainRegistry>
        -driver: Arc<dyn ContainerDriver>
        -cluster: Arc<ClusterController>
        -active_replicas: Arc<RwLock<HashMap>>
        +run_with_shutdown(member_rx, command_rx, shutdown_rx, ack_tx) Result
        +handle_member_down(peer)
        +handle_member_up(peer)
        +trigger_failover(node_id) Result<usize>
        +trigger_failback(domain) Result<bool>
    }

    class ContainerDriver {
        <<interface>>
        +spawn_container(domain, image, env, port) Future
        +stop_container(domain) Future
    }

    class DockerContainerDriver {
        -socket_path: String
        +spawn_container(...)
        +stop_container(...)
    }

    class MockContainerDriver {
        -spawned: Arc<Mutex<HashMap>>
        -stopped: Arc<Mutex<Vec<String>>>
    }

    WorkloadDuplicator --> ContainerDriver : Executes Container Actions
    DockerContainerDriver ..|> ContainerDriver : Implements Bollard Docker
    MockContainerDriver ..|> ContainerDriver : Implements Deterministic Mock
```
