# View D: End-to-End Data Flow Architecture

## 1. Primary Ingress Data Paths

BRIDGE supports three distinct data paths across its operational modes, plus a dedicated L4 UDP data path.

---

## 2. Mode 2: Handoff Mode Data Path (Default L4 SNI Passthrough)

In Handoff mode, Bridge operates at Layer 4. It peeks initial handshake bytes to extract the TLS SNI or HTTP Host header, resolves the target node via consistent hashing, optionally prepends a binary PROXY protocol v2 header, and transparently streams raw TCP payloads across the WireGuard mesh without decrypting TLS.

```mermaid
flowchart TD
    Client["Client / Browser"]
    IngressSocket["TCP Listener (:443 / :80)\nproxy/src/transport.rs"]
    WorkerPool["Elastic Worker Pool\nproxy/src/concurrency/pool.rs"]
    PeekBuffer["Initial Packet Peek (4096 bytes)\nproxy/src/handoff/mod.rs"]
    SniParser{"parse_tls_sni(&buf)\nByte Match: 0x16 0x03 ..."}
    HttpHostParser{"parse_http_host(&buf)\nRegex / String Scan"}
    RedirectCheck{"redirect_http == true && is_http?"}
    Http301Resp["HTTP/1.1 301 Moved Permanently\nLocation: https://{host}{uri}"]
    RegistryLookup["DomainRegistry::lookup(domain)\nArcSwap Lock-Free Read"]
    TargetResolve{"Multi-Target Route?"}
    RingLookup["HashRing::get_for_client\nToken = SHA256(ClientIP + Host)"]
    DirectTarget["Primary Node Target Address"]
    ProxyProtocolCheck{"send_proxy_protocol enabled?"}
    PrependPpv2["Prepend Binary PROXY v2 Header\n(16 bytes + IPv4/IPv6 Addrs)"]
    TargetConnect["TcpStream::connect(target_addr)\n(via WireGuard wg0: 10.8.0.x:443)"]
    ForwardInitial["Send Initial Buffer (4096 bytes)"]
    BidirectionalCopy["tokio::io::copy_bidirectional\nZero-Decryption TCP Stream Pipe"]
    LocalCoolify["Destination VM Coolify / Traefik\n(Terminates Let's Encrypt TLS)"]
    AppContainer["Target Docker Container (:port)"]

    %% Flow
    Client -->|"1. TCP SYN & Handshake"| IngressSocket
    IngressSocket -->|"2. Dispatched Task"| WorkerPool
    WorkerPool -->|"3. Read Initial Bytes"| PeekBuffer
    PeekBuffer --> SniParser

    SniParser -->|"SNI Extracted (e.g. app.example.com)"| RegistryLookup
    SniParser -->|"Not TLS"| HttpHostParser

    HttpHostParser -->|"Host Header Extracted"| RedirectCheck
    HttpHostParser -->|"Neither SNI nor Host"| DiscardNoRoute["Close TCP Stream (No Route)"]

    RedirectCheck -->|"Yes (HTTP->HTTPS Redirect)"| Http301Resp
    Http301Resp -->|"Flush & Close"| Client
    RedirectCheck -->|"No"| RegistryLookup

    RegistryLookup -->|"Route Found"| TargetResolve
    RegistryLookup -->|"Route Not Found"| Return404["Return 200/404 Plaintext & Close"]

    TargetResolve -->|"Yes (Multiple Nodes)"| RingLookup
    TargetResolve -->|"No (Single Node)"| DirectTarget
    RingLookup --> ProxyProtocolCheck
    DirectTarget --> ProxyProtocolCheck

    ProxyProtocolCheck -->|"Yes"| PrependPpv2
    ProxyProtocolCheck -->|"No"| TargetConnect
    PrependPpv2 --> TargetConnect

    TargetConnect --> ForwardInitial
    ForwardInitial --> BidirectionalCopy
    BidirectionalCopy <-->|"Transparent Encrypted TCP"| LocalCoolify
    LocalCoolify <-->|"Plaintext HTTP"| AppContainer
```

---

## 3. Mode 1: Direct Mode Data Path (L7 Reverse Proxy)

In Direct mode, Bridge acts as a Layer 7 HTTP reverse proxy. It terminates HTTP connections, inspects request headers, and forwards requests to backend services using Hyper's client connection pool.

```mermaid
flowchart TD
    Client["Client / API Consumer"]
    HttpListener["TCP Listener :80\nproxy/src/transport.rs"]
    HyperServer["Hyper HTTP/1.1 Server\nproxy/src/server/mod.rs"]
    HostNormalizer["normalize_host(Host Header)\nStrips :port and Lowercases"]
    RegistryLookup["DomainRegistry::lookup(host)"]
    ConnectUpgrade{"Method == CONNECT?"}
    TcpTunnel["tokio::io::copy_bidirectional\nDirect TCP Tunnel"]
    UpstreamFormat["Format Upstream URI\nhttp://{upstream}{path_and_query}"]
    HyperClientPool["Hyper Client Connection Pool\n(Reused TCP Keep-Alive Connections)"]
    WireguardMesh["WireGuard Mesh (10.8.0.x:port)"]
    BackendService["Backend Service / Web App"]
    MetricsCollector["DomainRegistry::record_request\nLatency Sample + Error Flag"]

    Client -->|"HTTP Request"| HttpListener
    HttpListener --> HyperServer
    HyperServer --> HostNormalizer
    HostNormalizer --> RegistryLookup

    RegistryLookup -->|"Found Route"| ConnectUpgrade
    RegistryLookup -->|"No Route"| Return503["HTTP 200 'No route found'"]

    ConnectUpgrade -->|"CONNECT (WebSocket/Tunnel)"| TcpTunnel
    ConnectUpgrade -->|"Standard GET/POST/..."| UpstreamFormat

    UpstreamFormat --> HyperClientPool
    HyperClientPool -->|"Proxied HTTP"| WireguardMesh
    WireguardMesh --> BackendService
    BackendService -->|"HTTP Response"| HyperClientPool
    HyperClientPool --> MetricsCollector
    MetricsCollector -->|"Return HTTP Response"| Client
```

---

## 4. Mode 3: Managed Mode Data Path (Traefik Provider Control Plane)

In Managed mode, Bridge is **completely removed from the application data hot path**. Bridge operates purely as a dynamic control-plane provider for Coolify's local Traefik proxy.

```mermaid
sequenceDiagram
    autonumber
    actor Client
    participant Traefik as Local Coolify Traefik (:443)
    participant BridgeFile as /data/coolify/proxy/dynamic/bridge.yaml
    participant BridgeDaemon as Bridge Daemon (Control Plane)
    participant Registry as DomainRegistry (Gossip Synced)
    participant RemoteNode as Remote VM (10.8.0.3)
    participant Container as Docker Application (:3000)

    Note over BridgeDaemon,Registry: Control Plane Synchronization Loop (every 5s)
    BridgeDaemon->>Registry: Snapshot active domain routes
    BridgeDaemon->>BridgeDaemon: Filter out local node routes (let local Coolify handle them)
    BridgeDaemon->>BridgeDaemon: Generate Traefik dynamic YAML (routers + loadBalancer servers)
    BridgeDaemon->>BridgeFile: Atomic write (.tmp -> rename)
    BridgeFile-->>Traefik: Traefik File Provider hot-reloads configuration in-memory

    Note over Client,Container: Application Request Path (Bridge is NOT in this path)
    Client->>Traefik: HTTPS GET app.example.com (Port 443)
    Traefik->>Traefik: Terminate TLS using local Let's Encrypt certificates
    Traefik->>Traefik: Match Router Rule: Host(`app.example.com`)
    Traefik->>RemoteNode: Proxy HTTP request across WireGuard mesh (http://10.8.0.3:3000)
    RemoteNode->>Container: Forward to local Docker container port
    Container-->>RemoteNode: HTTP 200 OK + payload
    RemoteNode-->>Traefik: Return response across WireGuard mesh
    Traefik-->>Client: HTTPS 200 OK (Encrypted)
```

---

## 5. Generic L4 UDP & QUIC Data Path

```mermaid
flowchart TD
    ClientUdp["Client UDP / QUIC Sender"]
    UdpListenerSock["UdpListener Service Socket\nproxy/src/udp/listener.rs"]
    SessionCheck{"Session exists for ClientAddr?"}
    CreateSession["spawn_session(client_addr, upstream)\nproxy/src/udp/session.rs"]
    OutboundSock["Ephemeral Connected Outbound UdpSocket\n(0.0.0.0:0 -> upstream)"]
    ForwardDatagram["Forward Datagram to Upstream"]
    UpstreamService["Upstream Service / Game Server / DNS\n(e.g. 10.8.0.3:53)"]
    ReceiveResponse["Recv Datagram from Outbound Socket"]
    ReturnToClient["Ingress Socket::send_to(resp, client_addr)"]
    IdleTimer{"Inactivity Timeout (30s)?"}
    ReapSession["Remove from SessionTable & Drop Sockets"]

    ClientUdp -->|"UDP Datagram"| UdpListenerSock
    UdpListenerSock --> SessionCheck
    SessionCheck -->|"Existing Session"| ForwardDatagram
    SessionCheck -->|"New Client"| CreateSession

    CreateSession --> OutboundSock
    OutboundSock --> ForwardDatagram
    ForwardDatagram --> UpstreamService
    UpstreamService -->|"UDP Response"| ReceiveResponse
    ReceiveResponse --> ReturnToClient
    ReturnToClient --> ClientUdp

    ForwardDatagram -.-> IdleTimer
    ReceiveResponse -.-> IdleTimer
    IdleTimer -->|"Timeout Expired"| ReapSession
```

---

## 6. State Mutability & Persistence Classification

| State Entity | Location / Ownership | Persistence Level | Mutability & Synchronization |
| :--- | :--- | :--- | :--- |
| **Domain Routes (`Route`)** | `DomainRegistry` ([`registry/src/lib.rs`](file:///home/peni/Projects/bridge/registry/src/lib.rs)) | In-memory with checkpointing to `bridge-state.json` on shutdown | Mutable via static config, Docker auto-discovery, IPC commands, and gossip `RouteSync`. |
| **Lamport Vector Clocks (`RouteClock`)** | `Route::clock` | In-memory; synced via gossip | Monotonically increasing `(origin_node, version)`. |
| **Peer Nodes (`PeerNode`)** | `ClusterController::peers` | In-memory; seeds persisted to `bridge.toml` | Updated via SWIM `MemberUp`/`MemberDown` and `JoinResponse`. |
| **WireGuard Peers Table** | `WireGuardDevice::peers` | In-memory; synced to kernel `wg0` | Updated when peers are registered or dropped; flushed via Netlink API. |
| **Leader Election Consensus** | `ElectionState` | Transient in-memory | Reset on election cycle; term increments monotonically. |
| **Active Container Replicas** | `WorkloadDuplicator::active_replicas` | Transient in-memory | Spawned on `MemberEvent::Down`; stopped on failback. |
| **Dashboard Session Tokens** | `AuthManager::sessions` | Ephemeral in-memory | Created on login; expires after 24 hours; lost on daemon restart. |
| **User Account Credentials** | `AuthConfig::users` in `bridge.toml` | **Persistent** on disk | Stored as Argon2 PHC hashes in configuration file. |
