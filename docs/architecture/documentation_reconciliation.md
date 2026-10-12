# Documentation-to-Code Reconciliation Report

This report compares every architectural claim made across existing repository documentation (`README.md`, `docs/PROXY_ARCHITECTURE.md`, `docs/PRESENTATION_RUNBOOK.md`, `website/docs/architecture.html`, and `website/docs/configuration.html`) against the actual implementation in the codebase.

Discrepancies are explicitly classified according to the audit rubric.

---

## 1. Summary of Discrepancies by Classification

```
┌─────────────────────────────────┬───────┐
│ Classification                  │ Count │
├─────────────────────────────────┼───────┤
│ Verified                        │  14   │
│ Partially Verified              │   4   │
│ Contradicted                    │   4   │
│ Undocumented Implementation     │   6   │
│ Documented but Unimplemented   │   2   │
│ Uncertain                       │   1   │
└─────────────────────────────────┴───────┘
```

---

## 2. Detailed Discrepancy Register

### Category A: Contradicted (Code Behaves Differently from Documentation)

#### 1. Consistent Hashing Virtual Nodes per Target: 360 vs. 128
* **Documented Claim:** [`README.md:59`](file:///home/peni/Projects/bridge/README.md#L59) asserts:
  > *"Consistent Hashing: SHA-256 ring with 128 virtual nodes per target for sticky session affinity."*
* **Actual Implementation:** [`registry/src/ring.rs:8-9`](file:///home/peni/Projects/bridge/registry/src/ring.rs#L8-L9) and [`registry/src/ring.rs:63-65`](file:///home/peni/Projects/bridge/registry/src/ring.rs#L63-L65):
  ```rust
  pub const DEFAULT_VNODES_PER_NODE: usize = 360;
  ```
  The code defines `DEFAULT_VNODES_PER_NODE = 360` with extensive comments explaining that 360 tokens provides optimal uniform distribution with low standard deviation. `docs/PRESENTATION_RUNBOOK.md:57` also references "360 vnodes".
* **Classification:** **Contradicted**
* **Recommended Correction:** Update `README.md` to state 360 virtual nodes per target instead of 128.

#### 2. Kernel `splice(2)` vs. Tokio Userspace Splicing
* **Documented Claim:** [`README.md:60`](file:///home/peni/Projects/bridge/README.md#L60) asserts:
  > *"Transparent TCP/TLS Splicing: Zero-copy kernel splicing (`splice(2)`) for high-throughput TLS SNI pass-through."*
* **Actual Implementation:** [`proxy/src/handoff/mod.rs:134`](file:///home/peni/Projects/bridge/proxy/src/handoff/mod.rs#L134):
  ```rust
  let (from_client, from_target) = tokio::io::copy_bidirectional(&mut client_stream, &mut target_stream).await?;
  ```
  There is no invocation of the Linux `splice(2)` system call anywhere in the codebase. All bidirectional streaming uses Tokio's asynchronous userspace buffer copying (`tokio::io::copy_bidirectional`).
* **Classification:** **Contradicted**
* **Recommended Correction:** Clarify in `README.md` and documentation that streaming uses high-performance asynchronous Tokio bidirectional streaming (`copy_bidirectional`), not zero-copy kernel `splice(2)`.

#### 3. Default Proxy Operating Mode: Handoff vs. Direct
* **Documented Claim:** [`website/docs/architecture.html:128-130`](file:///home/peni/Projects/bridge/website/docs/architecture.html#L128-L130) states:
  > *"L7 Direct ... This is the default mode and the one the examples use."*
* **Actual Implementation:** [`proxy/src/core/enums.rs:14-16`](file:///home/peni/Projects/bridge/proxy/src/core/enums.rs#L14-L16):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
  pub enum ProxyMode {
      Direct,
      #[default]
      Handoff,
      Managed,
  }
  ```
  The code derives `Default` on `ProxyMode` pointing to `Handoff`. Additionally, [`docs/PROXY_ARCHITECTURE.md:42`](file:///home/peni/Projects/bridge/docs/PROXY_ARCHITECTURE.md#L42) designates Handoff as the default. However, sample files ([`bridge-local.toml`](file:///home/peni/Projects/bridge/bridge-local.toml) and [`scripts/install.sh`](file:///home/peni/Projects/bridge/scripts/install.sh)) explicitly specify `mode = "Direct"`.
* **Classification:** **Contradicted** (Website contradicts code enum definition)
* **Recommended Correction:** Align `website/docs/architecture.html` with the codebase by noting that `Handoff` is the compile-time default enum variant in code, while minimal starter configs explicitly set `Direct`.

#### 4. Cluster Membership State Names: Foca SWIM vs. Website Documentation
* **Documented Claim:** [`website/docs/architecture.html:70-78`](file:///home/peni/Projects/bridge/website/docs/architecture.html#L70-L78) documents a 4-state lifecycle: `Alive → Suspected → Dead → Recovered`.
* **Actual Implementation:** [`cluster/src/controller.rs:22-24`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L22-L24) defines:
  ```rust
  pub enum MemberEvent {
      Up(PeerNode),
      Down(PeerNode),
  }
  ```
  Internally, Foca manages member states (Suspect, Down, Active), but the cluster controller surfaces only two discrete high-level events: `MemberEvent::Up` and `MemberEvent::Down`. Foca purges `Down` members after 2 seconds (`remove_down_after = Some(Duration::from_secs(2))`).
* **Classification:** **Partially Verified / Overgeneralized**
* **Recommended Correction:** Clarify that while Foca tracks internal suspect transitions, the cluster architecture surfaces binary `Up` and `Down` membership events to subsystems.

---

### Category B: Documented but Unimplemented Functionality

#### 1. L7 Direct Mode Automatic ACME RFC 8555 (`instant-acme`) & `rustls` TLS Termination
* **Documented Claim:** [`docs/PROXY_ARCHITECTURE.md:36`](file:///home/peni/Projects/bridge/docs/PROXY_ARCHITECTURE.md#L36) claims:
  > *"Bridge terminates TLS using `rustls` and provisions certificates automatically via ACME RFC 8555 (`instant-acme`)."*
* **Actual Implementation:** Neither `instant-acme` nor custom `rustls` TLS listener termination exists in `Cargo.toml` or the source tree. In Direct mode ([`proxy/src/server/handler.rs`](file:///home/peni/Projects/bridge/proxy/src/server/handler.rs)), the proxy accepts cleartext HTTP or tunnels `CONNECT` requests. Incoming TLS termination with automated ACME certificate issuance is completely absent from the codebase.
* **Classification:** **Documented but unimplemented**
* **Recommended Correction:** Remove claims of automated ACME RFC 8555 certificate management from `PROXY_ARCHITECTURE.md` or label them as "Planned Future Work / Specification Only".

#### 2. Tier 3b Ingress Handoff: Floating IP Failover
* **Documented Claim:** [`website/docs/architecture.html:171-174`](file:///home/peni/Projects/bridge/website/docs/architecture.html#L171-L174) documents:
  > *"floating_ip: A floating IP is reassigned to the new leader. Providers that offer reassignable floating IPs."*
* **Actual Implementation:** [`src/core/config.rs:126`](file:///home/peni/Projects/bridge/src/core/config.rs#L126) defines `HandoffMode::FloatingIp`, but there is zero implementation logic for floating IP reassignments in [`src/daemon.rs`](file:///home/peni/Projects/bridge/src/daemon.rs) or [`src/handoff/`](file:///home/peni/Projects/bridge/src/handoff/). Only `Tunnel` and `Dns` are implemented.
* **Classification:** **Documented but unimplemented**
* **Recommended Correction:** Mark `floating_ip` in documentation as "Reserved / Unimplemented" until cloud provider API integrations (Hetzner, DigitalOcean) are built.

---

### Category C: Undocumented Implementation (Functionality in Code but Missing in Docs)

#### 1. Generic L4 UDP Proxying & Port Mapping
* **Implementation Evidence:** [`proxy/src/udp/`](file:///home/peni/Projects/bridge/proxy/src/udp/) implements a full L4 UDP proxy service with ephemeral bidirectional session binding (`spawn_session`), session tables, inactivity timeout reapers, and configuration support under `[[udp_services]]`.
* **Documentation Status:** Missing from `README.md` and `website/docs/architecture.html`. Only present in unit tests and config parsing.
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Add a section on L4 UDP Proxying to the configuration and architecture documentation.

#### 2. QUIC Header Parsing & DCID-Based Connection Routing
* **Implementation Evidence:** [`proxy/src/udp/quic.rs`](file:///home/peni/Projects/bridge/proxy/src/udp/quic.rs) parses RFC 9000 Long and Short header frames, extracts Destination Connection IDs (DCIDs), and implements `QuicRouter` for persistent session stickiness across UDP datagrams.
* **Documentation Status:** Not mentioned in any official documentation or README.
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Document QUIC/HTTP3 L4 connection ID routing capabilities.

#### 3. Adaptive Elastic Concurrency Autoscaler (Work-Stealing Pool)
* **Implementation Evidence:** [`proxy/src/concurrency/pool.rs`](file:///home/peni/Projects/bridge/proxy/src/concurrency/pool.rs) and [`proxy/src/concurrency/monitor.rs`](file:///home/peni/Projects/bridge/proxy/src/concurrency/monitor.rs) implement crossbeam work-stealing, sliding window queue dwell monitoring, automatic scaling from 1 worker up to `max_concurrency` ceiling under load, and idle worker reaping. A dedicated binary [`src/bin/demo_autoscaling.rs`](file:///home/peni/Projects/bridge/src/bin/demo_autoscaling.rs) demonstrates this.
* **Documentation Status:** Mentioned in an asset diagram (`docs/assets/13_concurrency_scaling_architecture.png`), but omitted from `README.md` and user-facing architecture guides.
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Highlight Bridge's adaptive concurrency architecture in the system overview.

#### 4. Automatic WireGuard Key Generation & Config Persistence
* **Implementation Evidence:** [`src/core/config.rs:421-522`](file:///home/peni/Projects/bridge/src/core/config.rs#L421-L522) automatically generates missing private/public keys and writes them back to `bridge.toml` or `bridge.yaml` on initial boot. Discovered peer nodes are also automatically persisted into the `seeds` section.
* **Documentation Status:** Mentioned in installation scripts, but omitted from `README.md` configuration details.
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Document self-healing and zero-config bootstrap behavior in configuration guides.

#### 5. User Management & Argon2 Authentication CLI
* **Implementation Evidence:** [`src/cli.rs:142-146`](file:///home/peni/Projects/bridge/src/cli.rs#L142-L146) implements `bridge user create`, `bridge user list`, `bridge user passwd` commands communicating over IPC to `AuthManager`.
* **Documentation Status:** Omitted from the CLI command table in `README.md`.
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Add `bridge user` subcommands to the `README.md` CLI reference table.

#### 6. Dynamic Inspection & Administrative Cluster Controls
* **Implementation Evidence:** Commands `bridge inspect <domain>`, `bridge replicas`, `bridge replicate`, `bridge failback`, `bridge proxy status`, and `bridge pruneroutes` are implemented in `src/cli.rs`.
* **Documentation Status:** Omitted from the `README.md` CLI table (only present in presentation runbook).
* **Classification:** **Undocumented implementation**
* **Recommended Correction:** Update `README.md` CLI Reference table with complete command inventory.

---

### Category D: Verified Implementation Claims

| Architectural Feature | Documentation Source | Implementation Reference | Status |
| :--- | :--- | :--- | :--- |
| **WireGuard L3 Mesh Overlay** | `README.md:45-56`, `docs/PROXY_ARCHITECTURE.md:186` | `mesh/src/wireguard.rs:77-220` (`WireGuardDevice`) | **Verified** |
| **SWIM Failure Detection** | `README.md:57`, `website/docs/architecture.html:56` | `cluster/src/controller.rs:95-96`, `foca` integration | **Verified** |
| **Bully Leader Election & Term Monotonicity** | `README.md:58`, `cluster/src/election.rs` | `cluster/src/controller.rs:564-637` | **Verified** |
| **Dynamic Majority Quorum Guard** | `docs/PRESENTATION_RUNBOOK.md:15` | `cluster/src/election.rs:57-68` (`(total / 2) + 1`) | **Verified** |
| **L4 SNI Extraction without Decryption** | `docs/PROXY_ARCHITECTURE.md:50`, `README.md:52` | `proxy/src/handoff/sni.rs:1-120` (`parse_tls_sni`) | **Verified** |
| **Coolify / Docker Label Auto-Discovery** | `README.md:171-185`, `website/docs/architecture.html:60` | `src/discovery/docker.rs:51-135` | **Verified** |
| **Mode 3 Managed Mode (Traefik Dynamic YAML Injection)** | `docs/PROXY_ARCHITECTURE.md:83-165` | `proxy/src/managed/mod.rs:107-360` | **Verified** |
| **Workload Duplication on Node Failure** | `README.md:61`, `docs/PRESENTATION_RUNBOOK.md:17` | `src/failover/duplicator.rs:294-430` | **Verified** |
| **Preemptive, NonPreemptive & Manual Failback Modes** | `docs/PRESENTATION_RUNBOOK.md:17` | `src/core/config.rs:229-239`, `src/failover/duplicator.rs` | **Verified** |
| **Tier 2: Cloudflare DNS Record API Failover** | `website/docs/architecture.html:161-163` | `src/handoff/dns.rs:99-235` | **Verified** |
| **Tier 3a: Cloudflare Tunnel Dynamic Spawning** | `website/docs/architecture.html:166-168` | `src/handoff/tunnel.rs:14-118` | **Verified** |
| **Unix Domain Socket IPC Administration** | `README.md:101-104`, `README.md:143-168` | `src/ipc/uds.rs:17-1650` | **Verified** |
| **Embedded Web Observability Dashboard** | `README.md:97-100`, `src/dashboard/mod.rs` | `src/dashboard/mod.rs:64-150` | **Verified** |
| **In-Place GitHub Release Auto-Updater with SHA256** | `README.md:165`, `src/updater.rs` | `src/updater.rs:137-220` | **Verified** |

---

### Category E: Uncertain Behavior

#### 1. NAT Traversal Reliability for Dynamic WireGuard Endpoints
* **Issue:** WireGuard peers configure persistent keepalive (25 seconds). However, when two nodes behind symmetric NAT attempt direct peering without an external STUN/TURN coordinator or fixed public endpoints, connection establishment depends on raw UDP hole punching, which is not guaranteed by `defguard_wireguard_rs`.
* **Code Location:** [`mesh/src/wireguard.rs:30`](file:///home/peni/Projects/bridge/mesh/src/wireguard.rs#L30) (`persistent_keepalive: 25`).
* **Classification:** **Uncertain** (Runtime behavior depends on ISP/firewall NAT types).
