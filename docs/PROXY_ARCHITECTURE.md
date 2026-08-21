# BRIDGE Proxy Architecture & Routing Modes

> **Document Version:** 0.4 — Living Document  
> **Component:** `bridge-proxy` (Rust / Tokio / WireGuard Mesh)  
> **Status:** Active Research & Implementation Specification

---

## 1. Overview & Architectural Philosophy

Bridge is designed as a distributed, fault-tolerant ingress routing mesh across multi-cloud VPS fleets (Hetzner, DigitalOcean, Vultr, AWS, bare-metal). 

At the proxy layer, Bridge solves multi-server domain routing without creating a single point of failure (SPOF) while coexisting seamlessly with existing node-level deployment platforms like **Coolify**, **Traefik**, and **Nginx**.

Bridge provides two distinct proxy operation modes:

```
MODE 1 (Direct):   domain ──────► Bridge (L7) ─────────────────────────► VM ──────────────► service
MODE 2 (Handoff):  domain ──────► Bridge (L4 SNI Router) ──────────────► VM's Coolify ───► service
                                   [Zero TLS Termination / Passthrough]    [Local L7 Proxy]
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

## 5. Configuration Example (`bridge.toml`)

```toml
[node]
id          = "vm-01"
endpoint    = "65.21.100.1:51820"
listen_port = 51820

# ── Proxy Layer Configuration ─────────────────────────────────
[proxy]
mode = "handoff"                      # "handoff" (default, SNI L4 router) | "direct" (L7 reverse proxy)
listen = "0.0.0.0:443"
health_check_interval_s = 10

# ── Domain Registry Mappings ──────────────────────────────────
[domains]
"app.example.com"       = "vm-03"     # Forward SNI app.example.com -> VM-03:443 (Coolify)
"api.example.com"       = "vm-07"     # Forward SNI api.example.com -> VM-07:443 (Coolify)
"dashboard.example.com" = "vm-03"     # Forward SNI dashboard.example.com -> VM-03:443 (Coolify)
"auth.example.com"      = "vm-02"     # Forward SNI auth.example.com -> VM-02:443 (Coolify)

# ── Handoff HA Configuration ──────────────────────────────────
[handoff]
mode = "tunnel"                       # "tunnel" | "floating_ip" | "dns" | "none"

[handoff.tunnel]
tunnel_id    = "cf-tunnel-uuid"
secret       = "env:CF_TUNNEL_SECRET"
warm_standby = true
```

---

## 6. Rust Data Models & Types

In [`proxy/src/core/enums.rs`](file:///home/peni/Projects/bridge/proxy/src/core/enums.rs):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// Direct mode: Bridge terminates TLS and acts as an L7 reverse proxy.
    Direct,
    /// Handoff mode: Bridge acts as an SNI-based L4 transparent passthrough router.
    #[default]
    Handoff,
}
```

---

## 7. Open Questions & Implementation Considerations

1. **PROXY Protocol v2 Support:**  
   When Bridge forwards raw TCP connections to a destination VM's Coolify proxy over WireGuard, the destination proxy sees the connection originating from Bridge's WireGuard IP (e.g. `10.8.0.1`). Bridge should optionally prepend a **PROXY protocol v2 header** before streaming the `ClientHello` bytes so that Traefik/Nginx can recover real client source IPs for rate limiting, geo-blocking, and logging.
2. **Plain HTTP (Port 80) Handling in Handoff Mode:**  
   For non-TLS requests on port 80, Bridge can either:
   - Perform an automatic `301 Moved Permanently` redirect to `https://<Host>/` at the entry point.
   - Inspect the HTTP `Host:` header in plaintext and forward the TCP stream to the target VM's port 80.
3. **Coolify Webhook / Gossip Integration:**  
   Coolify can notify the local Bridge daemon on container deploy/stop events via a lightweight localhost HTTP webhook (`POST /v1/routes`), instantly broadcasting route changes across the fleet.
