# BRIDGE Proxy Architecture & Routing Modes

> **Document Version:** 0.4 — Living Document  
> **Component:** `bridge-proxy` (Rust / Tokio / WireGuard Mesh)  
> **Status:** Active Research & Implementation Specification

---

## 1. Overview & Architectural Philosophy

Bridge is designed as a distributed, fault-tolerant ingress routing mesh across multi-cloud VPS fleets (Hetzner, DigitalOcean, Vultr, AWS, bare-metal). 

At the proxy layer, Bridge solves multi-server domain routing without creating a single point of failure (SPOF) while coexisting seamlessly with existing node-level deployment platforms like **Coolify**, **Traefik**, and **Nginx**.

Bridge provides three distinct operation modes:

```
MODE 1 (Direct):   Client ──────► Bridge (L7) ─────────────────────────► VM ──────────────► service
MODE 2 (Handoff):  Client ──────► Bridge (L4 SNI Router) ──────────────► VM's Coolify ───► service
                                   [Zero TLS Termination / Passthrough]    [Local L7 Proxy]
MODE 3 (Managed):  Bridge ──────► configures Coolify Proxy (Traefik)
                   Client ──────► Coolify Proxy ────────────────────────────────────────────► service
                                   [Bridge is control-plane only — zero hot-path involvement]
```

![Proxy Modes Architecture](assets/09_proxy_modes_architecture.png)

---

## 2. Proxy Modes Specification

### 2.1 Mode 1 — Direct Mode (L7 Reverse Proxy)

* **Data Path:** `domain → Bridge → VM → service`
* **OSI Layer:** Layer 7 (Application / HTTP)
* **TLS Handling:** Bridge **terminates TLS** using `rustls` and provisions certificates automatically via ACME RFC 8555 (`instant-acme`).
* **Routing Logic:** Bridge parses inbound HTTP/1.1 and HTTP/2 requests, inspects HTTP `Host` headers, request paths, and headers, and executes reverse proxy forwarding directly to backend container ports across the WireGuard mesh (e.g. `10.8.0.3:3000`).
* **Use Case:** Bare VPS fleets running standalone Docker containers or native systemd services without local reverse proxies.

---

### 2.2 Mode 2 — Handoff Mode (SNI-Based L4 Passthrough Router) ★ *Default & Experimental*

* **Data Path:** `domain → Bridge → VM's Coolify proxy → service`
* **OSI Layer:** Layer 4 (Transport / TCP Stream Router)
* **TLS Handling:** **Zero TLS Termination.** Bridge does not hold private keys or SSL certificates for customer domains. TLS is terminated end-to-end at the destination VM's Coolify proxy.
* **HTTP Handling:** Bridge does not parse, modify, or buffer HTTP payloads.

#### Handoff Mode Mechanics
1. **TLS ClientHello Peek / SNI Extraction:** When a client initiates a TLS connection to port 443, Bridge inspects the initial bytes of the TLS handshake record without completing the handshake, extracting the **Server Name Indication (SNI)** hostname (e.g., `app.example.com`).
2. **Domain Registry Lookup:** Bridge performs an $O(1)$ lock-free lookup in its local Domain Registry table (e.g., `app.example.com → VM-03`).
3. **Transparent L4 Forwarding (TCP Stream Passthrough):** Bridge opens a TCP connection over the encrypted WireGuard mesh to `VM-03:443` and transparently splices bidirectional TCP traffic (via zero-copy `tokio::io::copy_bidirectional`).
4. **Local Coolify Proxy Routing:** `VM-03`'s local Coolify proxy (Traefik / Caddy / Nginx) terminates TLS using its locally managed Let's Encrypt certificates, matches its internal Docker routing labels, and reverse-proxies to the local application container.

```mermaid
sequenceDiagram
    autonumber
    actor Client
    participant Bridge as Bridge Leader (L4 Router)
    participant Registry as Domain Registry
    participant Coolify as VM-03 Coolify (:443)
    participant App as App Container

    Client->>Bridge: TCP SYN & Handshake on port 443
    Client->>Bridge: TLS ClientHello (SNI: app.example.com)
    Note over Bridge: Peek ClientHello bytes<br/>(No TLS decryption / No private keys)
    Bridge->>Registry: LookupDomain("app.example.com")
    Registry-->>Bridge: VM-03 (10.8.0.3:443)
    Bridge->>Coolify: WireGuard TCP Connect + forward ClientHello
    Note over Bridge,Coolify: Zero-copy TCP stream splicing (L4 passthrough)
    Coolify-->>Client: TLS ServerHello + Local Certificate (Let's Encrypt)
    Note over Client,Coolify: End-to-end TLS session established directly
    Client->>Coolify: Encrypted HTTPS GET /api/v1 (via Bridge L4 pipe)
    Coolify->>App: Decrypt & proxy to local container port
    App-->>Coolify: HTTP 200 OK + payload
    Coolify-->>Client: Encrypted HTTPS Response (via Bridge L4 pipe)
```

![SNI Handoff Sequence](assets/10_seq_sni_handoff_routing.png)

---

### 2.3 Mode 3 — Managed Mode (Coolify Proxy Dynamic Routing)

* **Control Path:** `Bridge → Traefik Provider API → Coolify Proxy routing config`
* **Data Path:** `Client → Coolify Proxy (Traefik) → service`
* **Bridge on Hot Path:** **No.** Bridge does not bind listener ports, terminate TLS, or touch application traffic.
* **TLS Handling:** Entirely delegated to Coolify's Traefik instance (Let's Encrypt / local certificates).

#### Core Idea

In Direct and Handoff modes, Bridge sits in the request data path — it either terminates TLS (Direct) or splices TCP streams (Handoff). Managed mode takes a fundamentally different approach: Bridge **never touches application traffic**. Instead, it extends the routing configuration of the existing Coolify proxy (Traefik) running on each VM.

Coolify already deploys Traefik as its local reverse proxy, which discovers services via Docker labels and manages TLS certificates. The limitation is that Traefik only knows about containers on its local Docker host. For cross-node routing (e.g., `app.example.com` resolving to VM-01 but the service running on VM-03), Traefik has no native awareness.

Bridge closes this gap by acting as a **dynamic Traefik configuration provider**, injecting routers, services, and middleware that make Traefik route cross-node traffic over the WireGuard mesh — without Bridge ever being in the request path.

#### Managed Mode Mechanics

1. **Domain Registry as Source of Truth:** Bridge maintains its fleet-wide Domain Registry (`domain → VM`) via gossip, exactly as in other modes.
2. **Traefik Provider Integration:** On each VM, the local Bridge daemon translates Domain Registry entries into Traefik dynamic configuration and pushes them via Traefik's [file provider](https://doc.traefik.io/traefik/providers/file/) (writing YAML/TOML to a watched directory) or [HTTP provider](https://doc.traefik.io/traefik/providers/http/) (serving an endpoint that Traefik polls).
3. **Cross-Node Route Injection:** For a domain mapped to a remote VM, Bridge generates a Traefik service pointing to the remote VM's WireGuard IP (e.g., `http://10.8.0.3:3000`), allowing Traefik to proxy the request over the encrypted mesh.
4. **Local Routes Untouched:** Domains mapped to the local VM are already handled by Coolify's own Docker label discovery. Bridge skips these — no configuration conflict.

#### Example: Traefik Dynamic Configuration (Generated by Bridge)

```yaml
# Auto-generated by Bridge daemon — written to /etc/traefik/dynamic/bridge.yml
# Traefik watches this file via its file provider.

http:
  routers:
    bridge-app-example:
      rule: "Host(`app.example.com`)"
      service: bridge-app-example
      tls:
        certResolver: letsencrypt
      entryPoints:
        - websecure

  services:
    bridge-app-example:
      loadBalancer:
        servers:
          - url: "http://10.8.0.3:3000"    # VM-03 via WireGuard mesh
```

#### Sequence: Managed Mode Request Flow

```mermaid
sequenceDiagram
    autonumber
    participant Bridge as Bridge Daemon (Control Plane)
    participant Registry as Domain Registry (Gossip)
    participant Traefik as Coolify Proxy / Traefik
    participant Config as Traefik Dynamic Config
    actor Client
    participant App as App Container (VM-03)

    Note over Bridge,Registry: Startup / Route Change
    Registry-->>Bridge: Route update: app.example.com → VM-03
    Bridge->>Config: Write bridge.yml (router + service for app.example.com → 10.8.0.3:3000)
    Config-->>Traefik: File provider hot-reload

    Note over Client,App: Request Flow (Bridge is NOT in the path)
    Client->>Traefik: HTTPS GET app.example.com
    Traefik->>Traefik: TLS termination (Let's Encrypt)
    Traefik->>App: HTTP proxy → 10.8.0.3:3000 (WireGuard)
    App-->>Traefik: HTTP 200 OK
    Traefik-->>Client: HTTPS Response
```

![Managed Mode Architecture](assets/12_managed_mode_architecture.png)

#### Trade-offs vs. Handoff Mode

| Dimension | Handoff Mode | Managed Mode |
| :--- | :--- | :--- |
| **Bridge on hot path** | Yes (L4 TCP splicing) | No (control-plane only) |
| **Failure blast radius** | Bridge failure = traffic outage | Bridge failure = stale routes (Traefik keeps serving last-known config) |
| **Latency overhead** | Extra hop through Bridge + WireGuard | Direct to Traefik (single WireGuard hop for cross-node) |
| **TLS certificate management** | Destination VM's Coolify | Local Traefik (same VM that receives traffic) |
| **Complexity** | Bridge must handle TCP stream lifecycle | Bridge must integrate with Traefik's provider API |
| **Operational dependency** | Requires Bridge to be running and healthy for live traffic | Traefik operates independently once configured |

---

## 3. Architectural Separation of Concerns

Bridge and Coolify play complementary, strictly decoupled roles in the infrastructure stack:

| Dimension | Bridge (Global Traffic Router) | Coolify Proxy (Local Service Router) |
| :--- | :--- | :--- |
| **Scope** | Global fleet entry-point (Cross-node / Cross-provider) | Local single-node router (Intra-node) |
| **OSI Layer** | **Layer 4 (Transport / SNI Passthrough)** | **Layer 7 (Application / HTTP Reverse Proxy)** |
| **TLS / SSL** | Transparent passthrough (Does not hold domain certs) | Local TLS termination (Manages Let's Encrypt certs) |
| **Service Discovery**| Fleet-wide Domain Registry (`domain → VM`) gossiped via SWIM | Local Docker socket label scanning (`service → container`) |
| **Failover / HA** | High-availability leader election & tunnel handoff | Local container health checks & zero-downtime restarts |
| **Status** | Stable Core Routing Layer | Experimental Local Handoff Integration |

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                   GLOBAL ENTRY-POINT & FLEET TRAFFIC ROUTER (BRIDGE)                   │
│  • Public Anycast / Cloudflare Tunnel / Floating IP Ingress                            │
│  • Leader Election & Failover Detection (SWIM Gossip)                                  │
│  • WireGuard Encrypted L3 Mesh Interconnect                                            │
│  • Zero-Overhead SNI-Based L4 Passthrough Router Engine                                │
│  • Fleet Domain Registry: app.example.com → VM-03, api.example.com → VM-07             │
└───────────────────────────────────────────┬────────────────────────────────────────────┘
                                            │
               WireGuard Mesh: Transparent L4 TCP Passthrough (:443)
                                            │
       ┌────────────────────────────────────┼────────────────────────────────────┐
       ▼                                    ▼                                    ▼
┌───────────────┐                    ┌───────────────┐                    ┌───────────────┐
│     VM-03     │                    │     VM-07     │                    │     VM-12     │
│ (Hetzner FSN) │                    │ (DigitalOcean)│                    │ (Vultr / Bare)│
│               │                    │               │                    │               │
│ Coolify Proxy │                    │ Coolify Proxy │                    │ Coolify Proxy │
│ (Traefik:443) │                    │ (Traefik:443) │                    │ (Traefik:443) │
│   │ local SSL │                    │   │ local SSL │                    │   │ local SSL │
│   ▼           │                    │   ▼           │                    │   ▼           │
│ App Container │                    │ API Container │                    │Admin Container│
│(app.example)  │                    │(api.example)  │                    │(admin.example)│
└───────────────┘                    └───────────────┘                    └───────────────┘
```

![Architectural Separation](assets/11_proxy_handoff_separation.png)

---

## 4. Domain Registry & Routing Table

The Domain Registry maps inbound fully qualified domain names (FQDNs) to target VM node identities:

```toml
# In-memory Domain Registry mappings:
"app.example.com"       => "vm-03"    # (WireGuard Endpoint: 10.8.0.3:443)
"api.example.com"       => "vm-07"    # (WireGuard Endpoint: 10.8.0.7:443)
"dashboard.example.com" => "vm-03"    # (WireGuard Endpoint: 10.8.0.3:443)
"auth.example.com"      => "vm-02"    # (WireGuard Endpoint: 10.8.0.2:443)
```

### Registration & Propagation
1. **Static Configuration:** Defined in `bridge.toml` under `[domains]`.
2. **Dynamic Gossip Sync:** When a node registers or modifies a local domain route, it emits a gossip event. All peer nodes synchronize the routing table in $O(\log N)$ gossip rounds.
3. **Lock-Free Fast Path:** The active leader stores the compiled routing table in an `ArcSwap<DomainRegistry>`, enabling concurrent, lock-free routing reads with sub-microsecond lookup latency.

---

## 5. Adaptive Concurrency & Dynamic Worker Model

Bridge implements an adaptive, event-driven worker concurrency engine inspired by Cloudflare's **Pingora** and Tokio's multi-threaded work-stealing scheduler.

Instead of statically allocating fixed worker pools or spawning unbounded tasks per connection, Bridge uses an **elastic worker pool with work-stealing deques** that scales dynamically based on ingress queue dwell time.

```
                  ┌───────────────────────────────────────────────────────────┐
                  │                 INGRESS & DISPATCH LAYER                  │
                  │  • Single Acceptor Loop (TCP Listener on :443 / :80)      │
                  │  • Ingress Work-Stealing Deque (crossbeam-deque SPMC)     │
                  │  • Queue Dwell Latency Monitor (Sliding 20-Req Window)    │
                  └─────────────────────────────┬─────────────────────────────┘
                                                │
                 ┌──────────────────────────────┼──────────────────────────────┐
                 ▼                              ▼                              ▼
        ┌──────────────────┐           ┌──────────────────┐           ┌──────────────────┐
        │  Worker Task 1   │◄──────────│  Worker Task 2   │◄──────────│  Worker Task N   │
        │(Baseline Active) │work-steal │(Auto-Spawned #2) │work-steal │(Up to Max Limit) │
        │                  │──────────►│                  │──────────►│                  │
        │ • SNI Peek / L4  │           │ • SNI Peek / L4  │           │ • Reaps on idle  │
        │ • TCP Splicing   │           │ • TCP Splicing   │           │   cooldown > 15s │
        └──────────────────┘           └──────────────────┘           └──────────────────┘
```

![Adaptive Concurrency & Work-Stealing Architecture](assets/13_concurrency_scaling_architecture.png)

---

### 5.1 Architecture & Core Components

1. **Ingress Acceptor Loop (Single Producer):**
   - The primary TCP listener loop accepts inbound TCP connections and immediately hands them off to the ingress dispatch layer with zero blocking.
2. **Work-Stealing Deques (`crossbeam-deque` SPMC):**
   - Each worker task maintains a local double-ended queue (deque).
   - **Local FIFO/LIFO:** The worker pushes and pops from the bottom of its own queue for cache locality.
   - **Work-Stealing:** When a worker runs out of tasks, it steals work from the top (FIFO) of other workers' deques or the global injector queue. This eliminates head-of-line blocking and spreads bursty load evenly across active workers.
3. **Elastic Worker Pool (1 to `max_concurrency`):**
   - **Baseline:** Bridge starts with a minimal footprint of **1 worker task** (`tokio::spawn`), consuming near-zero idle CPU and memory.
   - **Scale-Up:** Under load, Bridge spawns additional worker tasks on demand up to `max_concurrency`.
   - **Scale-Down:** When traffic drops, idle worker tasks gracefully terminate after a cooldown period, shrinking back down to the baseline of 1 worker.

---

### 5.2 Adaptive Auto-Scaling Algorithm

The autoscaler uses **Queue Dwell Time (Queuing Latency)** over a sliding request buffer rather than raw connection counts to detect processing bottlenecks:

#### Metrics & Timing Mechanics

1. **Timestamp Tagging:** When an incoming connection is accepted, it is stamped with its arrival timestamp: $t_{\text{accepted}} = \text{Instant::now()}$.
2. **Dwell Time Computation:** When an available worker dequeues the connection to begin SNI extraction, it computes the queue wait duration:
   $$\Delta t_{\text{dwell}} = t_{\text{start}} - t_{\text{accepted}}$$
3. **Sliding Buffer (20 Requests):** The metrics monitor records $\Delta t_{\text{dwell}}$ across a rolling window of the last 20 requests.

#### Scaling Rules

* **Scale-Up Trigger:**
  If the average queue dwell time across the rolling buffer exceeds the latency threshold ($\tau_{\text{dwell}} = 5\text{ms}$), or if a batch of 20 requests experiences persistent queue backlog:
  $$\overline{\Delta t}_{\text{dwell}} > \tau_{\text{dwell}} \quad \text{and} \quad N_{\text{workers}} < N_{\text{max\_concurrency}}$$
  Bridge immediately spawns a new worker task (`tokio::spawn`) to drain the queue.
* **Scale-Down / Reaper Trigger:**
  When a dynamically spawned worker finds its local and steal deques empty for an idle cooldown timeout ($T_{\text{cooldown}} = 15\text{s}$), the worker exits cleanly:
  $$T_{\text{idle}} \ge 15\text{s} \quad \text{and} \quad N_{\text{workers}} > 1 \implies \text{Worker Graceful Exit}$$

---

### 5.3 Concurrency Limits & Hardware Auto-Detection

The maximum concurrency ceiling is controlled by `max_concurrency`:

* **Manual Override:** Explicitly set in `bridge.toml` under `[proxy.max_concurrency]`.
* **Default Ceiling:** Defaults to `20` worker tasks for general VPS workloads.
* **System Auto-Detection Helper:** When set to `"auto"` (or left unconfigured on multi-core systems), Bridge auto-tunes the concurrency ceiling based on hardware specifications:
  $$N_{\text{max}} = \min\left(64, \; \max\left(4, \; N_{\text{logical\_cpus}} \times 2\right)\right)$$

---

## 6. Configuration Example (`bridge.toml`)

```toml
[node]
id          = "vm-01"
endpoint    = "65.21.100.1:51820"
listen_port = 51820

# ── Proxy Layer Configuration ─────────────────────────────────
[proxy]
mode            = "handoff"           # "handoff" (default, SNI L4) | "direct" (L7) | "managed" (control-plane)
listeners       = ["https"]
redirect_http   = false
max_concurrency = 20                  # Maximum concurrent worker tasks (default: 20, or "auto")

# ── Target Nodes (WireGuard Endpoints) ────────────────────────
[[proxy.nodes]]
node_id  = "vm-03"
endpoint = "10.8.0.3:443"

[[proxy.nodes]]
node_id  = "vm-07"
endpoint = "10.8.0.7:443"

# ── Domain-to-Node Routing Table ─────────────────────────────
[[services]]
url     = "https://app.example.com"
node_id = "vm-03"

[[services]]
url     = "https://api.example.com"
node_id = "vm-07"
```

---

## 7. Rust Data Models & Types

In [`proxy/src/core/enums.rs`](file:///home/peni/Projects/bridge/proxy/src/core/enums.rs):

```rust
/// Operating modes for the Bridge proxy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProxyMode {
    /// Direct mode: Bridge terminates TLS and acts as an L7 reverse proxy.
    Direct,
    /// Handoff mode (Default): Bridge acts as an SNI-based L4 transparent passthrough router.
    #[default]
    Handoff,
    /// Managed mode: Bridge does not proxy application traffic. Instead it acts as a
    /// control-plane component that dynamically configures the existing Coolify proxy (Traefik).
    Managed,
}
```

In [`src/core/config.rs`](file:///home/peni/Projects/bridge/src/core/config.rs):

```rust
#[derive(Clone, SmartDefault, Deserialize)]
#[serde(default)]
pub struct Config {
    #[default(false)]
    pub enable_telemetry: bool,
    pub sentry: SentryConfig,
    pub logger: LoggerConfig,
    pub proxy: ProxyConfig,
    pub services: Vec<Services>,
}

#[derive(Clone, SmartDefault, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub mode: proxy::core::enums::ProxyMode,
    pub listeners: Vec<proxy::core::enums::Scheme>,
    #[default(false)]
    pub redirect_http: bool,
    pub nodes: Vec<registry::Node>,
    /// Maximum worker task concurrency ceiling (default: 20)
    #[default(20)]
    pub max_concurrency: usize,
}
```

---

## 8. Open Questions & Implementation Considerations

1. **Work-Stealing vs Single SPMC Channel Benchmark:**  
   Benchmarking `crossbeam-deque` against `tokio::sync::mpsc` under high connection churn (>50,000 req/s) to ensure lock-free deques offer measurable throughput and latency gains on low-core VPS instances.
2. **PROXY Protocol v2 Support:**  
   When Bridge forwards raw TCP connections to a destination VM's Coolify proxy over WireGuard, Bridge should optionally prepend a **PROXY protocol v2 header** before streaming the `ClientHello` bytes so that Traefik/Nginx can recover real client source IPs for rate limiting, geo-blocking, and logging.
3. **Plain HTTP (Port 80) Handling in Handoff Mode:**  
   For non-TLS requests on port 80, Bridge can either perform an automatic `301 Moved Permanently` redirect to `https://<Host>/` at the entry point or inspect the plaintext HTTP `Host:` header and forward the TCP stream to the target VM's port 80.
4. **Coolify Webhook / Gossip Integration:**  
   Coolify can notify the local Bridge daemon on container deploy/stop events via a lightweight localhost HTTP webhook (`POST /v1/routes`), instantly broadcasting route changes across the fleet.

