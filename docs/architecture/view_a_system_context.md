# View A: System Context Architecture

## 1. System Boundary and Purpose

**BRIDGE** is a distributed, fault-tolerant edge mesh proxy, service discoverer, and failover orchestrator for multi-cloud and bare-metal VPS fleets.

Its primary purpose is to transform a cluster of heterogeneous Linux servers running containerized workloads (such as Docker, Coolify, Traefik, or native systemd services) into a single, self-healing, unified ingress fabric. Inbound traffic reaching any entry point in the fleet is dynamically routed to the appropriate backend service across an encrypted WireGuard overlay mesh, with automatic leader failover and container replica migration upon server failure.

---

## 2. System Context Diagram

```mermaid
flowchart TD
    %% Human Actors
    subgraph Actors ["Human Actors & Administrators"]
        ClientUser["End-User / Web Client\n(Browser / Mobile / API Consumer)"]
        Operator["Fleet Operator / DevOps Engineer\n(CLI Admin / Dashboard User)"]
    end

    %% External Infrastructure & Services
    subgraph ExternalServices ["External Cloud Services & Infrastructure"]
        CloudflareDNS["Cloudflare DNS API\n(RFC 1035 / REST v4)"]
        CloudflareEdge["Cloudflare Edge Anycast\n(Public Entrypoint)"]
        DockerEngine["Local Docker Daemon\n(/var/run/docker.sock)"]
        GitHubRelease["GitHub Releases API\n(Release Binaries & Checksums)"]
        SentryMonitoring["Sentry Telemetry\n(Error & Performance Tracing)"]
    end

    %% BRIDGE System Boundary
    subgraph BridgeFleet ["BRIDGE SYSTEM FLEET BOUNDARY (10.8.0.0/24 Overlay Mesh)"]
        LeaderNode["BRIDGE Node (Elected Leader)\n• Edge Ingress Receiver\n• Bully Election Coordinator\n• Cloudflare Handoff Controller"]
        FollowerNodeA["BRIDGE Node (Follower Node)\n• Local Service Exposer\n• SWIM Gossip Peer\n• Workload Replica Host"]
        FollowerNodeB["BRIDGE Node (Follower Node)\n• Local Service Exposer\n• SWIM Gossip Peer\n• Workload Replica Host"]
    end

    %% Managed Local Services
    subgraph LocalWorkloads ["Managed Node Applications & Local Proxies"]
        TraefikProxy["Coolify Local Traefik Proxy\n(:80 / :443 TLS Termination)"]
        AppContainers["Containerized Applications\n(Docker / Web Services / APIs)"]
    end

    %% Interactions
    ClientUser -->|"HTTPS / TLS (Port 443)\nHTTP (Port 80)\nUDP (L4 Services)"| CloudflareEdge
    CloudflareEdge -->|"Ingress Traffic\n(Direct / Tunnel / Public IP)"| LeaderNode

    Operator -->|"Administrative Commands\n(/tmp/bridge.sock Unix Socket)"| LeaderNode
    Operator -->|"Observability UI & WS Stream\n(HTTP :9090 / Auth: Argon2)"| LeaderNode

    LeaderNode <-->|"SWIM Epidemic Gossip & Vector Clocks\n(Raw UDP / BRDG Protocol)"| FollowerNodeA
    LeaderNode <-->|"SWIM Epidemic Gossip & Vector Clocks\n(Raw UDP / BRDG Protocol)"| FollowerNodeB
    FollowerNodeA <-->|"SWIM Gossip & State Sync"| FollowerNodeB

    LeaderNode <-->|"WireGuard Encrypted L3 Mesh\n(wg0 / 10.8.0.x / ChaCha20-Poly1305)"| FollowerNodeA
    LeaderNode <-->|"WireGuard Encrypted L3 Mesh\n(wg0 / 10.8.0.x / ChaCha20-Poly1305)"| FollowerNodeB

    LeaderNode -.->|"A-Record Patch\n(REST API Token)"| CloudflareDNS
    LeaderNode -.->|"cloudflared Tunnel Run\n(CLI Child Process)"| CloudflareEdge
    LeaderNode -.->|"Crash Reports & Metrics\n(HTTPS)"| SentryMonitoring

    LeaderNode -->|"Inspect Container Labels & Events"| DockerEngine
    FollowerNodeA -->|"Inspect Container Labels & Events"| DockerEngine
    FollowerNodeB -->|"Inspect Container Labels & Events"| DockerEngine

    LeaderNode -.->|"In-Place Update Check\n(HTTPS Releases)"| GitHubRelease

    LeaderNode -->|"L4 TCP Stream / PROXY v2\n(Over WireGuard wg0)"| TraefikProxy
    FollowerNodeA -->|"Dynamic YAML File\n(Managed Mode)"| TraefikProxy
    TraefikProxy -->|"HTTP Forwarding (:port)"| AppContainers
    LeaderNode -->|"Direct HTTP/UDP Forwarding"| AppContainers
```

---

## 3. Boundary Components & External Interfaces

### 3.1 External Actors
* **End-User / Client Applications:** Send HTTP, HTTPS (TLS), and generic UDP datagrams targeting fleet-hosted domains (e.g. `app.example.com`, `api.example.com`).
* **Fleet Operator / Administrator:** Interacts with the daemon locally via the administrative CLI (`bridge` commands over Unix Domain Socket) or through the browser via the embedded Web Observability Dashboard (`http://127.0.0.1:9090`).

### 3.2 External Cloud Systems & Infrastructure
* **Cloudflare Anycast Edge & Tunnel (`cloudflared`):** Routes global ingress into the fleet without opening inbound ports or exposing public IPs on edge servers.
* **Cloudflare DNS (REST API v4):** Receives automated A-record updates from the elected cluster leader when operating in Tier 2 DNS failover mode.
* **Local Docker Engine (`/var/run/docker.sock`):** Discovered by the daemon via Bollard to stream container lifecycle events (`start`, `stop`, `die`) and extract routing labels (`traefik.http.routers.*`, `coolify.domain`, `bridge.domain`). Also used by the workload failover duplicator to spawn container replicas.
* **GitHub Releases API (`https://api.github.com/repos/Penivera/Bridge`):** Queried by the in-place self-updater to download static musl binaries, verify SHA-256 digests, and perform atomic executable replacement.
* **Sentry Telemetry Platform:** Receives runtime exception captures, panic backtraces, and performance tracing events when telemetry is explicitly enabled (`enable_telemetry = true`).

---

## 4. Trust Boundaries & Communication Security

| Boundary | Mechanism & Protocol | Authentication & Authorization | Confidentiality & Integrity |
| :--- | :--- | :--- | :--- |
| **Public Edge Ingress** | TCP / UDP (Ports 80, 443, custom UDP) | TLS ClientHello SNI inspection; PROXY protocol v2 | TLS 1.3 encrypted end-to-end between client and destination VM proxy |
| **Inter-Node Fleet Mesh** | WireGuard Linux Kernel Interface (`wg0`) | Curve25519 static public keys | ChaCha20-Poly1305 symmetric authenticated encryption |
| **Cluster Protocol & Gossip** | Raw UDP datagrams (`BRDG` wire format) | 4-byte magic + 1-byte version; Foca SWIM identity | Overlay network encapsulation; optional WireGuard transport |
| **Local Host IPC** | Unix Domain Socket (`/tmp/bridge.sock`) | Local file permissions (`0750` / `0640`); root/bridge user | Host kernel IPC memory isolation |
| **Web Dashboard** | HTTP / WebSocket (`127.0.0.1:9090`) | Argon2id PHC password hash; Bearer / Cookie session tokens | Localhost binding by default; leader-only public ingress |
| **Container Engine** | Unix Socket (`/var/run/docker.sock`) | Docker group / host root socket access | Host filesystem security |
