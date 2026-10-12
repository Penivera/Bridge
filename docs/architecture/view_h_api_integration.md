# View H: API and Integration Architecture

## 1. Overview of Interfaces

BRIDGE exposes and integrates across five distinct interface boundaries:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        BRIDGE INTEGRATION LAYERS                       │
└────────────────────────────────────────────────────────────────────────┘
  1. Administrative IPC API     ──► Unix Domain Socket (/tmp/bridge.sock)
  2. Web Observability REST/WS  ──► HTTP / WebSocket (127.0.0.1:9090)
  3. Cluster Gossip Wire API    ──► Raw UDP Datagrams (Port 51820 / BRDG Wire)
  4. Container Engine API       ──► Docker Engine Unix Socket (/var/run/docker.sock)
  5. Cloudflare External APIs   ──► Cloudflare DNS REST v4 & cloudflared CLI
```

---

## 2. Unix Domain Socket IPC API (`/tmp/bridge.sock`)

The IPC interface provides full administrative control over the running daemon. It uses **JSON lines** over a stream-oriented Unix domain socket.

### Request Format
Requests are formatted as single-line JSON objects tagged with `"action"`:
```json
{"action": "add_route", "domain": "api.example.com", "upstream": "127.0.0.1:8080", "node_id": "vm-02"}
```

### Response Format
Responses return a JSON object with `"status": "ok"` or `"status": "error"`:
```json
{"status": "ok", "domain": "api.example.com", "action": "added"}
```

### Complete IPC Action Inventory

| Action Name | Parameters | Response Payload | Description |
| :--- | :--- | :--- | :--- |
| `ping` | None | `{"status": "ok", "message": "pong"}` | Liveness health check. |
| `status` | None | `IpcData::Status` | Version, uptime, routes count, proxy mode, leader state. |
| `health` | None | `IpcData::HealthSummary` | Detailed system, cluster, node, and route health statistics. |
| `list_routes` | None | `IpcData::Routes` | Snapshot of all registered routes, targets, and upstreams. |
| `add_route` | `domain`, `upstream?`, `node_id?` | `{"domain": "...", "action": "added"}` | Dynamically registers or updates an ingress route. |
| `remove_route` | `domain` | `{"domain": "...", "action": "removed"}` | Evicts a route from the domain registry. |
| `prune_routes` | None | `{"pruned_count": N}` | Prunes routes targeting evicted or dead nodes. |
| `cluster_status` | None | `IpcData::ClusterStatus` | Local node rank, leader, term, quorum, and active peer list. |
| `mesh_status` | None | `IpcData::MeshDevice` | WireGuard interface name, public key, port, and peer list. |
| `mesh_sync` | None | `{"status": "ok", "peers_synced": N}` | Forces kernel WireGuard interface synchronization. |
| `election_status`| None | `IpcData::ElectionInfo` | Current leader, term, role, quorum, and received acks. |
| `trigger_election`| None | `{"status": "ok", "started": true}` | Triggers an immediate Bully election round. |
| `set_leader` | `node_id` | `{"status": "ok", "leader": "...", "term": N}` | Administratively force-appoints a cluster leader. |
| `step_down` | None | `{"status": "ok", "stepped_down": true}` | Administratively steps down from leadership. |
| `drop_node` | `node_id` | `{"status": "ok", "dropped_node": "..."}` | Evicts a node from cluster peers and WireGuard device. |
| `inspect_domain` | `domain`, `client_ip?` | `IpcData::DomainInspect` | Displays target nodes, hash ring tokens, and sticky affinity. |
| `list_replicas` | None | `IpcData::ReplicasList` | Lists active duplicated failover container workloads. |
| `trigger_replication` | `node_id` | `{"status": "ok", "spawned": N}` | Manually triggers failover duplication for a node. |
| `failback` | `domain` | `{"status": "ok", "domain": "..."}` | Manually triggers failback for a replicated domain. |
| `get_config` | None | `IpcData::ConfigView` | Returns active parsed configuration as JSON. |
| `reload_config` | None | `{"status": "ok", "reloaded": true}` | Hot-reloads configuration from disk. |
| `list_users` | None | `{"users": ["admin", ...]}` | Lists registered dashboard usernames. |
| `create_user` | `username`, `password` | `{"status": "ok", "user": "..."}` | Hashes password with Argon2id and adds user. |
| `set_password` | `username`, `password` | `{"status": "ok", "user": "..."}` | Updates user password hash with Argon2id. |
| `list_discovery`| None | `IpcData::DiscoveryList` | Lists auto-discovered Docker containers and labels. |

---

## 3. Web Observability Dashboard REST & WebSocket API (`127.0.0.1:9090`)

Built on Axum 0.8 with embedded static assets and session authentication.

### REST Endpoints

| Method | Path | Auth Required | Description |
| :--- | :--- | :--- | :--- |
| `GET` | `/` / `/ui` | Session / None | Serves embedded Single Page Application (`index.html`). |
| `GET` | `/assets/{*path}` | Public | Serves embedded CSS, JS, and PNG assets with correct MIME types. |
| `POST` | `/api/v1/login` | Public | Authenticates credentials with Argon2; issues session cookie & token. |
| `POST` | `/api/v1/logout`| Session | Invalidates current session token. |
| `GET` | `/api/v1/status` | Session / None | Daemon status, node ID, uptime, routes count, leader flag. |
| `GET` | `/api/v1/nodes` | Session / None | Participating cluster nodes, endpoints, and health status. |
| `GET` | `/api/v1/nodes/{id}`| Session / None | Detailed telemetry for a specific node and its hosted routes. |
| `GET` | `/api/v1/routes` | Session / None | Active routing table entries, upstreams, targets, and latency metrics. |
| `GET` | `/api/v1/mesh` | Session / None | WireGuard interface configuration, public key, and connected peers. |
| `GET` | `/api/v1/metrics` | Session / None | Fleet traffic metrics: total requests, errors, and P50/P95 latencies. |
| `GET` | `/api/v1/events` | Session / None | Recent ring-buffer events (membership changes, elections, failovers). |
| `GET` | `/api/v1/config` | Session / None | Active configuration file view in JSON format. |
| `PUT` | `/api/v1/config` | Admin Session | Updates configuration on disk and hot-reloads daemon. |
| `POST` | `/api/v1/replicate`| Admin Session | Triggers manual workload failover duplication for a failed node. |
| `POST` | `/api/v1/failback` | Admin Session | Triggers failback restoration for a duplicated domain. |

### Real-Time WebSocket Endpoint: `/api/v1/stream`
* **Protocol:** RFC 6455 WebSocket.
* **Authentication:** Validated via `token` query parameter or HTTP session cookie during initial upgrade.
* **Transmission:** Server streams JSON events over the WebSocket:
  * Periodic metric ticks (every 1000ms): `{"type": "metrics", "data": { ... }}`
  * Real-time cluster events: `{"type": "event", "data": { "event_type": "MEMBER_DOWN", ... }}`

---

## 4. Cluster Gossip Wire API (Raw UDP Datagrams)

Communicates over raw UDP sockets bound to port 51820 (or configured `listen_port`).

### Packet Framing
```
0                   1                   2                   3
0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       'B'     |       'R'     |       'D'     |       'G'     | Magic Bytes
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Version    |               Bincode Payload ...             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```
* **Magic Bytes:** `b"BRDG"` (4 bytes).
* **Protocol Version:** `0x01` (1 byte).
* **Payload:** Compact binary Bincode serialization of `ClusterMessage` enum variants.

---

## 5. External Cloud Integrations

### 5.1 Cloudflare DNS API v4
* **Transport:** HTTPS REST API (`https://api.cloudflare.com/client/v4`).
* **Authentication:** Bearer token via `CF_DNS_API_TOKEN` or `handoff.dns.api_token`.
* **Operations:**
  * `GET /zones/{zone_id}/dns_records?type=A&name={record_name}`
  * `PATCH /zones/{zone_id}/dns_records/{record_id}` (updates A-record to target IP)
  * `POST /zones/{zone_id}/dns_records` (creates A-record if absent)

### 5.2 Cloudflare Tunnel (`cloudflared`) CLI Process
* **Transport:** Host process management via `tokio::process::Command`.
* **Execution:** Spawns `cloudflared tunnel run --token <token>` as a supervised child process on leader election; gracefully terminates via `SIGTERM` / `SIGKILL` on leadership loss.

### 5.3 Docker Engine Socket
* **Transport:** Unix Domain Socket (`/var/run/docker.sock`).
* **Client Library:** `bollard::Docker` (API version: 1.43+).
* **Operations:**
  * `GET /containers/json` (initial container scan)
  * `GET /events` (streaming container lifecycle events)
  * `POST /containers/create` & `POST /containers/{id}/start` (failover container spawn)
  * `POST /containers/{id}/stop` & `DELETE /containers/{id}` (failback replica termination)
