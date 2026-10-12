# View J: Deployment, Operations, and Failure Architecture

## 1. Build, Packaging, and CI/CD Pipeline

BRIDGE is built using automated continuous integration in GitHub Actions, targeting fully static Linux binaries with zero glibc or shared library dependencies.

```mermaid
flowchart LR
    subgraph GitRepo ["Source Repository (GitHub: Penivera/Bridge)"]
        GitCommit["Push to main Branch\n(commit SHA)"]
    end

    subgraph GithubActions ["GitHub Actions CI Workflow (.github/workflows/release.yml)"]
        CrossBuildX86["cross build --release\nTarget: x86_64-unknown-linux-musl"]
        CrossBuildArm["cross build --release\nTarget: aarch64-unknown-linux-musl"]
        PackTarball["Tarball Packaging\nbridge + LICENSE + install.sh"]
        GenSha256["sha256sum Generation\n(sha256sums.txt)"]
        GhRelease["Publish GitHub Release\n(Tag: v0.1.0-{sha})"]
    end

    subgraph Distribution ["Edge VM Deployment & Auto-Update"]
        InstallerScript["scripts/install.sh\n(curl | sh)"]
        AutoUpdater["bridge update CLI\n(src/updater.rs)"]
        SystemdService["systemd Unit\n(bridge.service)"]
    end

    GitCommit --> CrossBuildX86
    GitCommit --> CrossBuildArm
    CrossBuildX86 --> PackTarball
    CrossBuildArm --> PackTarball
    PackTarball --> GenSha256
    GenSha256 --> GhRelease

    GhRelease --> InstallerScript
    GhRelease --> AutoUpdater
    InstallerScript --> SystemdService
    AutoUpdater --> SystemdService
```

---

## 2. Startup Ordering & Dependency Readiness Gates

When the `bridge` daemon boots (`systemd` start or manual execution), it executes a strict sequential readiness initialization sequence:

```mermaid
sequenceDiagram
    autonumber
    participant Init as Systemd / Process Start
    participant Config as Config::load_auto
    participant Telemetry as Telemetry & Sentry
    participant Registry as DomainRegistry
    participant Shutdown as ShutdownCoordinator
    participant Docker as DockerDiscovery
    participant IPC as IpcServer
    participant Dash as DashboardServer
    participant Mesh as WireGuard Kernel Interface
    participant Cluster as ClusterController
    participant Proxy as Proxy Listeners

    Init->>Config: Discover config file (CLI -> BRIDGE_CONFIG -> /etc/bridge/bridge.toml)
    Config-->>Init: Parsed Config Struct
    Init->>Telemetry: Initialize tracing-subscriber & Sentry SDK
    Init->>Registry: Populate static routes from config services
    Init->>Shutdown: Initialize ShutdownCoordinator (state_file: bridge-state.json)

    opt Docker Discovery Enabled
        Init->>Docker: Connect /var/run/docker.sock & start container event watcher
    end

    opt IPC Enabled
        Init->>IPC: Bind Unix Domain Socket (/tmp/bridge.sock)
    end

    opt Dashboard Enabled
        Init->>Dash: Bind HTTP / WebSocket Server (127.0.0.1:9090)
    end

    opt Node Configuration Present
        Init->>Mesh: Generate Curve25519 keys if absent, sync wg0 to kernel
        Init->>Cluster: Bind raw UDP socket (0.0.0.0:51820)
        Cluster->>Cluster: Send JoinRequest to configured seeds
        Note over Cluster: Wait 1500ms for JoinResponse<br/>(If timeout: proceed as independent node)
    end

    Init->>Proxy: Bind TCP listeners (:80, :443) & UDP listeners
    Note over Proxy,Cluster: Daemon enters running steady state
```

---

## 3. Failure Detection, Propagation, and Recovery Matrix

| Failure Event | Detection Mechanism | Time to Detect | System Reaction & Recovery Path | Impact on Ingress Traffic |
| :--- | :--- | :--- | :--- | :--- |
| **Ingress Leader Dies** | SWIM direct ping timeout (150ms) + indirect ping-req failure | $\approx 1000\text{--}1500\text{ms}$ | Surviving nodes initiate Bully election. Highest-priority node declares coordinator, verifies quorum, and assumes leadership. | Traffic to dead leader fails briefly until Cloudflare Tunnel takes over or DNS A-record updates. |
| **Worker VM (Follower) Dies** | SWIM direct probe fails; suspect-to-down timer expires (1000ms) | $1000\text{ms}$ | Controller emits `MemberEvent::Down`. Peer evicted from `wg0`. Ingress leader identifies replicated services and spawns container replicas on surviving nodes. | Multi-target traffic automatically fails over to surviving targets on the hash ring. Replicated services migrate to surviving VM. |
| **Transient Network Flap** | SWIM probe misses 1 ping; suspect state activated | $150\text{ms}$ | Indirect ping-req sent through random peers. If peer answers, node transitions back to `Alive` without triggering failover. | Zero traffic disruption; flapping damped by incarnation counters. |
| **Split-Brain Network Partition** | Bully election quorum guard: `(total / 2) + 1` | $300\text{ms}$ | Minority partition candidates fail quorum verification (`has_quorum == false`) and step down to `Follower`. Only majority partition elects leader. | Prevents split-brain DNS update thrashing. Minority partition stops serving as ingress entrypoint. |
| **Backend Container Dies** | Docker socket event stream emits `die` / `stop` event | $< 50\text{ms}$ | `DockerDiscovery` intercepts event, looks up associated domains, and calls `DomainRegistry::remove(domain)`. Route pruned immediately. | Subsequent connections receive 200 "No route found" or fail over to alternative ring targets. |
| **Daemon Process Abrupt Kill** | Systemd unit watchdog / process exit | Immediate | Systemd restarts daemon after `RestartSec=3`. On boot, daemon loads `bridge-state.json` and config seeds to restore mesh topology. | Temporary pause during the 3-second systemd restart window. |

---

## 4. Graceful Shutdown & Teardown Protocol

```mermaid
sequenceDiagram
    autonumber
    actor Admin as OS / Operator (SIGINT / SIGTERM)
    participant Coord as ShutdownCoordinator
    participant TCP as TCP / UDP Listeners
    participant Pool as Elastic Worker Pool
    participant Cluster as ClusterController (SWIM)
    participant Finalizer as State Finalizer Task
    participant Disk as bridge-state.json

    Admin->>Coord: Signal received (SIGINT or SIGTERM)
    Coord->>Coord: broadcast_and_wait(ShutdownReason, timeout: 10s)

    par Stop Accepting New Ingress Traffic
        Coord->>TCP: Shutdown broadcast
        TCP->>TCP: Close listener sockets (stop accepting)
        TCP->>Pool: Drain active workers
        Pool-->>Coord: ShutdownAck { peak_workers, remaining }
    and Broadcast SWIM Leave to Cluster
        Coord->>Cluster: Shutdown broadcast
        Cluster->>Cluster: Foca leave_cluster()
        Cluster->>Cluster: Send raw UDP Leave datagrams to all peers
        Cluster-->>Coord: ShutdownAck
    and Persist Runtime State Checkpoint
        Coord->>Finalizer: Shutdown broadcast
        Finalizer->>Finalizer: Snapshot DomainRegistry routes & UDP services
        Finalizer->>Disk: Atomic write to bridge-state.json (status: gracefully_stopped)
        Finalizer-->>Coord: ShutdownAck { routes_count }
    end

    Note over Coord: All subsystems acknowledged within 10s timeout
    Coord->>Admin: Daemon exits cleanly with exit code 0
```
