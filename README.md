<div align="center">

# BRIDGE

**Distributed. Self-aware. Fault-tolerant ingress for dynamic cloud infrastructure.**

[![Release](https://img.shields.io/github/v/release/Penivera/Bridge?color=blue&logo=github)](https://github.com/Penivera/Bridge/releases)
[![Build Status](https://img.shields.io/github/actions/workflow/status/Penivera/Bridge/release.yml?branch=main&logo=github)](https://github.com/Penivera/Bridge/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Documentation](https://img.shields.io/badge/Docs-bridgemesh.space-green?logo=cloudflare)](https://bridgemesh.space)
[![Rust](https://img.shields.io/badge/Built_with-Rust_1.80+-orange?logo=rust)](https://www.rust-lang.org)

[Website & Docs](https://bridgemesh.space) • [Getting Started](https://bridgemesh.space/docs/getting-started.html) • [Architecture](https://bridgemesh.space/docs/architecture.html) • [Releases](https://github.com/Penivera/Bridge/releases)

</div>

---

### Why Bridge?

I built this while working IT at the State Revenue Service. We had several servers running different services, but routing traffic to them was pure friction. Every deployment meant updating a stack of configurations across the board.

So I built **Bridge**.

Bridge is a lightweight daemon (emphasis on *lightweight*) that runs on your VMs. It watches services spin up and down, builds an automatic routing table across your fleet, handles automatic cross-VM replication, and makes every node aware of every service, no matter where it lives.

* **Plugs into your existing stack:** Traefik, Nginx, Caddy, or whatever you already run.
* **Cross-fleet routing:** One entry point, multiple servers.
* **Bridges the brain:** Makes a fleet of VMs work like one coordinated system.
* **First-class Coolify support:** Discovers Coolify Docker containers automatically and routes traffic between servers without manual reverse proxy changes.

---

### Core Architecture

```
Internet / Client Requests
          │
          ▼ (Ports 80 / 443)
┌────────────────────────────────────────────────────────┐
│               VM 1 (Elected Ingress Leader)            │
│  BRIDGE Daemon (Proxy + SWIM Gossip + WireGuard)        │
└───────────────┬────────────────────────┬───────────────┘
                │                        │
       WireGuard L3 Mesh        WireGuard L3 Mesh
       (10.8.0.1 <-> 10.8.0.2)   (10.8.0.1 <-> 10.8.0.3)
                │                        │
                ▼                        ▼
┌───────────────────────────┐ ┌───────────────────────────┐
│           VM 2            │ │           VM 3            │
│  BRIDGE (Follower Node)   │ │  BRIDGE (Follower Node)   │
│  Local Services (Docker)  │ │  Local Services (Docker)  │
└───────────────────────────┘ └───────────────────────────┘
```

* **WireGuard L3 Overlay:** Nodes establish a private peer-to-peer encrypted mesh (`10.8.0.x`) with automatic key distribution and kernel routing (`wg0`).
* **SWIM Epidemic Gossip:** Rapid, low-overhead failure detection (sub-second ping/ack heartbeats with indirect ping-req).
* **Bully Leader Election:** Deterministic, priority-weighted consensus elect an ingress edge leader for public DNS handoff.
* **Consistent Hashing:** SHA-256 ring with 128 virtual nodes per target for sticky session affinity.
* **Transparent TCP/TLS Splicing:** Zero-copy kernel splicing (`splice(2)`) for high-throughput TLS SNI pass-through.
* **Automated Workload Duplication:** If a node dies, the cluster leader detects it and orchestrates standby container replica failover.

---

### Quick Install

Run this command on each VM you want in your fleet:

```bash
curl -fsSL https://bridgemesh.space/install.sh | sh
```

*(Direct GitHub fallback: `curl -fsSL https://raw.githubusercontent.com/Penivera/Bridge/main/scripts/install.sh | sh`)*

The installer:
1. Detects your CPU architecture (`x86_64` or `aarch64`) and downloads the verified static musl binary.
2. Creates an unprivileged `bridge` system user with `CAP_NET_BIND_SERVICE` and `CAP_NET_ADMIN`.
3. Sets up `/etc/bridge` (mode `0750`) with an initial configuration.
4. Enables and starts the `bridge.service` systemd unit.

---

### Quickstart: 3-Node Fleet

#### 1. Configure the bootstrap seed node (`node-01` on `203.0.113.10`)
Edit `/etc/bridge/bridge.toml`:

```toml
enable_telemetry = false

[proxy]
mode = "Direct"
listeners = ["http", "https"]
http_addr = "0.0.0.0:80"
https_addr = "0.0.0.0:443"

[dashboard]
enabled = true
listen_addr = "127.0.0.1:9090"

[ipc]
enabled = true
socket_path = "/tmp/bridge.sock"

[node]
id = "node-01"
mesh_ip = "10.8.0.1"
endpoint = "203.0.113.10:51820"
listen_port = 51820
priority = 200
```

#### 2. Join additional nodes (`node-02` on `203.0.113.11`)
Configure `node-02` with `node-01` as its seed:

```toml
[node]
id = "node-02"
mesh_ip = "10.8.0.2"
endpoint = "203.0.113.11:51820"
listen_port = 51820
seeds = ["203.0.113.10:51820"]
priority = 100
```

#### 3. Verify fleet status
```bash
# Check overall health
bridge health

# View participating cluster nodes
bridge nodes

# View active WireGuard peer tunnels
bridge mesh peers

# Inspect current Bully leader election state
bridge election status
```

---

### CLI Reference

Bridge includes a robust administrative CLI for operational control over IPC:

| Command | Category | Description |
| :--- | :--- | :--- |
| `bridge health` | Diagnostics | Run comprehensive health check on daemon, backends, and cluster. |
| `bridge nodes` | Cluster | List all cluster nodes, lifecycle states, roles, and priorities. |
| `bridge node inspect <id>` | Cluster | Deep-dive telemetry for a specific node (IP, endpoint, hosted routes). |
| `bridge node drop <id>` | Cluster | Administratively evict a node and prune its routes from the cluster. |
| `bridge mesh status` | Mesh | Inspect local WireGuard interface (`wg0`), public key, and port. |
| `bridge mesh peers` | Mesh | List connected WireGuard peer tunnels, endpoints, and AllowedIPs. |
| `bridge mesh sync` | Mesh | Force kernel WireGuard interface and routing table synchronization. |
| `bridge election status` | Consensus | Display current leader, local role, term, and quorum status. |
| `bridge election trigger` | Consensus | Trigger an immediate leader election cycle across the fleet. |
| `bridge election set-leader <id>` | Consensus | Administratively force appoint a leader across the cluster. |
| `bridge election step-down` | Consensus | Administratively step down from leadership on the local node. |
| `bridge routes` | Routing | List active routing table entries with upstream targets and health. |
| `bridge routes prune` | Routing | Prune dead or orphaned routes targeting evicted nodes. |
| `bridge add-route <domain> <upstream>` | Routing | Dynamically add an ingress route into the daemon registry. |
| `bridge remove-route <domain>` | Routing | Remove an ingress route dynamically. |
| `bridge config show` | Config | Display the currently active configuration in TOML format. |
| `bridge config reload` | Config | Hot-reload the configuration file without restarting the daemon. |

---

### Coolify Integration

When running alongside [Coolify](https://coolify.io), Bridge automatically discovers Docker containers deployed across your servers:

```toml
[discovery.docker]
enabled = true
poll_interval_secs = 5
socket_path = "/var/run/docker.sock"
traefik_dynamic_path = "/data/coolify/proxy/dynamic"
```

Bridge reads container domain labels (`coolify.managed`, `traefik.http.routers.*`), advertises them across the fleet via vector-clocked gossip, and transparently routes incoming client traffic across the WireGuard overlay to the server hosting the container.

---

### Building from Source

```bash
# Prerequisites: Rust 1.80+ and Cargo
git clone https://github.com/Penivera/Bridge.git
cd Bridge

# Build release binary
cargo build --release

# Run test suite
cargo test
```

---

### Documentation

Comprehensive guides, configuration references, and operational architecture documentation are available at [bridgemesh.space](https://bridgemesh.space):

* [Getting Started](https://bridgemesh.space/docs/getting-started.html)
* [System Architecture](https://bridgemesh.space/docs/architecture.html)
* [Configuration Guide](https://bridgemesh.space/docs/configuration.html)
* [Operations Manual](https://bridgemesh.space/docs/operations.html)

---

### License

BRIDGE is open-source software licensed under the [MIT License](LICENSE).
