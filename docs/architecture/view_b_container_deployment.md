# View B: High-Level Container & Deployment Architecture

## 1. Overview & Runtime Units

BRIDGE is deployed as a single, statically compiled Rust binary running as an unprivileged system daemon (`bridge.service`) under `systemd` across every participating VM in a fleet.

There are no external database dependencies (e.g., PostgreSQL, Redis, etcd, Consul). All state is held in-memory with periodic disk checkpoints and configuration file synchronizations.

---

## 2. Container & Process Deployment Diagram

```mermaid
flowchart TB
    subgraph HostSystem ["LINUX VIRTUAL MACHINE / BARE-METAL SERVER"]
        
        subgraph SystemdSupervisor ["Systemd Init & Process Supervisor"]
            BridgeDaemonUnit["bridge.service\n(User: bridge, AmbientCaps: CAP_NET_BIND_SERVICE, CAP_NET_ADMIN)"]
        end

        subgraph BridgeProcess ["BRIDGE OS PROCESS (Static musl Rust Binary: /usr/local/bin/bridge)"]
            
            subgraph ControlPlane ["Control Plane Subsystems"]
                DaemonMain["Daemon Lifecycle & Coordinator\n(Tokio Runtime)"]
                IpcServer["Unix Socket IPC Server\n(/tmp/bridge.sock)"]
                DashboardServer["Axum HTTP/WS Dashboard\n(127.0.0.1:9090)"]
                DockerWatcher["Bollard Docker Event Watcher\n(Task)"]
            end

            subgraph DistributedCluster ["Distributed Cluster Engine"]
                ClusterSocket["Raw UDP Gossip Socket\n(0.0.0.0:51820 or 51821)"]
                FocaSwim["Foca SWIM Failure Detector"]
                BullyElection["Bully Election Coordinator"]
                DuplicatorEngine["Workload Duplicator Engine"]
            end

            subgraph DataPlaneProxy ["Data Plane Proxy Engine"]
                ElasticPool["Elastic Worker Pool\n(Crossbeam Deque + Dwell Monitor)"]
                TcpListener80["TCP Listener :80 (HTTP / Redirect)"]
                TcpListener443["TCP Listener :443 (HTTPS / SNI Peek)"]
                UdpServices["Generic UDP Listeners (L4)"]
                TraefikProvider["Managed Mode Traefik Sync Task"]
            end

            subgraph MemoryState ["In-Memory State Stores"]
                DomainRegistryMem["Domain Registry (ArcSwap HashMap)"]
                ConsistentHashRing["HashRing (360 vnodes per target)"]
                PeerTableMem["Active Peer Nodes Table"]
            end
        end

        subgraph LocalHostServices ["Host Infrastructure & Co-located Services"]
            DockerDaemon["Docker Engine Daemon\n(/var/run/docker.sock)"]
            LocalTraefik["Coolify Traefik Proxy Container\n(Host Ports :80, :443)"]
            LocalContainers["Backend Application Containers\n(Bridge Network / Localhost)"]
            CloudflaredProc["cloudflared Process (Child Process)\n(Managed by TunnelManager)"]
        end

        subgraph KernelNetwork ["Linux Kernel Network Subsystem"]
            WgInterface["WireGuard Device Interface (wg0)\n(Kernel Netlink / 10.8.0.x/24)"]
            HostRoutingTable["Kernel Routing Table & IPTables"]
        end

        subgraph HostStorage ["Local Persistent Disk Storage"]
            ConfigToml["/etc/bridge/bridge.toml\n(0640 - Configuration & Seed Peers)"]
            StateJson["bridge-state.json\n(Runtime Snapshot Checkpoint)"]
            TraefikYaml["/data/coolify/proxy/dynamic/bridge.yaml\n(Traefik Dynamic YAML Rules)"]
        end
    end

    %% Wiring & Control Connections
    BridgeDaemonUnit -->|"Spawns & Monitors"| BridgeProcess
    DaemonMain -->|"Registers Subsystem"| IpcServer
    DaemonMain -->|"Registers Subsystem"| DashboardServer
    DaemonMain -->|"Registers Subsystem"| ClusterSocket
    DaemonMain -->|"Registers Subsystem"| ElasticPool

    DockerWatcher -->|"Streams Events"| DockerDaemon
    DockerWatcher -->|"Updates Routes"| DomainRegistryMem

    ClusterSocket <-->|"Foca Protocols"| FocaSwim
    ClusterSocket <-->|"Election / Acks"| BullyElection
    ClusterSocket <-->|"RouteSync / Digests"| DomainRegistryMem
    ClusterSocket -->|"Kernel Sync (WGApi)"| WgInterface

    TcpListener443 -->|"Dispatches Connection"| ElasticPool
    TcpListener80 -->|"Dispatches Connection"| ElasticPool
    ElasticPool -->|"Consults Route & Ring"| DomainRegistryMem
    ElasticPool -->|"L4 Spliced Stream"| WgInterface
    ElasticPool -->|"Local Forwarding"| LocalTraefik

    TraefikProvider -->|"Writes Dynamic Config"| TraefikYaml
    LocalTraefik -->|"Watches Dynamic Config"| TraefikYaml
    LocalTraefik -->|"Reverse Proxies"| LocalContainers

    DuplicatorEngine -->|"Spawns Failover Container"| DockerDaemon

    DaemonMain -.->|"Persists Seeds on MemberEvent::Up"| ConfigToml
    DaemonMain -.->|"Checkpoints on Shutdown"| StateJson
    CloudflaredProc -.->|"Spawns as Child (Cold/Warm Standby)"| DaemonMain
```

---

## 3. Technology Inventory & Architectural Role

| Runtime Unit / Technology | Crate / Library | Architectural Role | Network / Storage Boundary |
| :--- | :--- | :--- | :--- |
| **Tokio Asynchronous Engine** | `tokio 1.53` (full) | Multi-threaded async runtime managing all event loops, timers, I/O multiplexing, and task spawning. | Process memory / OS threads |
| **Hyper HTTP Core** | `hyper 1.6`, `hyper-util` | Client connection pooling and HTTP request proxying in Direct Mode (L7). | TCP sockets / Loopback |
| **Crossbeam Deque** | `crossbeam-deque 0.8` | Work-stealing injector, local worker deques, and stealers for elastic concurrency scaling. | In-process lock-free queues |
| **Foca SWIM Gossip Engine** | `foca 2.0` | Decentralized failure detection, direct/indirect heartbeats, and cluster membership tracking. | Raw UDP (`BRDG` datagrams) |
| **WireGuard Netlink Integration** | `defguard_wireguard_rs 0.12` | Creates and configures Linux kernel `wg0` network interface, adds peers, sets AllowedIPs and kernel routes. | Linux Netlink / Kernel space |
| **Cryptography Primitives** | `x25519-dalek 3.0`, `ring 0.17`, `argon2 0.6` | Curve25519 key generation, SHA-256 ring hashing, and Argon2id password hashing. | In-memory CPU operations |
| **Docker Engine Client** | `bollard 0.21` | Communicates over `/var/run/docker.sock` to inspect containers, stream lifecycle events, and spawn replicas. | Local Unix domain socket |
| **Embedded Web Server** | `axum 0.8`, `tower-http 0.6` | Serves dashboard static assets, JSON REST API, and WebSocket live telemetry streams. | Localhost `127.0.0.1:9090` |
| **Configuration & Serialization** | `toml 1.1`, `serde_yaml 0.9`, `serde_json 1.0`, `bincode 1.3` | Parses configuration files, serializes Traefik YAML, saves state checkpoints, and encodes binary UDP packets. | Local filesystem / Network buffers |

---

## 4. Multi-Node Deployment Topology

In a typical production deployment across 3 or more nodes:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        PUBLIC INTERNET / CLIENTS                       │
└────────────────────────────────────┬───────────────────────────────────┘
                                     │
                 ┌───────────────────┴───────────────────┐
                 │ Public DNS / Cloudflare Tunnel Ingress│
                 └───────────────────┬───────────────────┘
                                     ▼
┌────────────────────────────────────────────────────────────────────────┐
│ NODE 1: Ingress Edge Leader (e.g. Hetzner FSN1 - 203.0.113.10)         │
│ • BRIDGE Daemon (Priority: 200, Role: Leader)                          │
│ • Binds Port 80, 443 (Handoff Mode)                                    │
│ • WireGuard Mesh IP: 10.8.0.1 (Interface: wg0)                         │
│ • Raw UDP Gossip Port: 51820                                           │
└───────────────────┬────────────────────────────────┬───────────────────┘
                    │                                │
    Encrypted WireGuard Mesh (10.8.0.1 <-> 10.8.0.2) │ Encrypted WireGuard Mesh (10.8.0.1 <-> 10.8.0.3)
                    │                                │
                    ▼                                ▼
┌──────────────────────────────────────┐ ┌───────────────────────────────┐
│ NODE 2: Worker Follower              │ │ NODE 3: Worker Follower       │
│ (DigitalOcean FRA1 - 203.0.113.11)   │ │ (Vultr AMS - 203.0.113.12)    │
│ • BRIDGE Daemon (Priority: 100)      │ │ • BRIDGE Daemon (Priority: 50)│
│ • WireGuard Mesh IP: 10.8.0.2        │ │ • WireGuard Mesh IP: 10.8.0.3 │
│ • Coolify Proxy (Traefik: 10.8.0.2:443│ │ • Docker App: 10.8.0.3:3000   │
│ • Application: app.example.com       │ │ • Application: api.example.com│
└──────────────────────────────────────┘ └───────────────────────────────┘
```

### Communication Topology
* **Public Ingress:** Handled exclusively by the elected Ingress Leader.
* **Control Plane (SWIM & Gossip):** Full peer-to-peer epidemic mesh over raw UDP (`0.0.0.0:51820`). Every node communicates directly with configured seeds and gossips membership and route digests to random peers (fanout=3).
* **Data Plane (Overlay Interconnect):** Point-to-point peer tunnels over Linux WireGuard interface `wg0` (`10.8.0.0/24`). Traffic routed between nodes travels directly between host kernel interfaces without centralized bottlenecks.
