# Architecture Decision and Uncertainty Register

This document records the architectural choices confirmed by implementation evidence, unresolved implementation ambiguities, and areas where runtime behavior cannot be established solely through static analysis.

---

## 1. Confirmed Architectural Decisions (Backed by Code Evidence)

### ADR-01: Layer 4 SNI Passthrough (Handoff Mode) as Primary Ingress Pattern
* **Context:** In multi-cloud and VPS deployments (e.g. Hetzner, DigitalOcean, bare-metal), applications frequently run behind localized reverse proxies (Coolify, Traefik, Caddy) that manage their own Let's Encrypt SSL certificates. Terminating TLS at an external mesh ingress creates double certificate management overhead and security key exposure.
* **Decision:** Implement transparent Layer 4 SNI-based TCP stream forwarding ([`proxy/src/handoff/mod.rs:27-146`](file:///home/peni/Projects/bridge/proxy/src/handoff/mod.rs#L27-L146)). Bridge inspects the initial TLS ClientHello bytes to extract the SNI hostname, routes traffic across the WireGuard mesh to the destination VM's port 443, and splices the connection without holding customer private keys.
* **Evidence:** [`proxy/src/handoff/sni.rs:1-120`](file:///home/peni/Projects/bridge/proxy/src/handoff/sni.rs#L1-L120), [`proxy/src/handoff/proxy_protocol.rs:38-164`](file:///home/peni/Projects/bridge/proxy/src/handoff/proxy_protocol.rs#L38-L164).
* **Trade-offs:** Low CPU overhead and zero TLS certificate management on Bridge, but prevents Layer 7 request inspection (e.g., path-based routing or header modification) in Handoff mode.

### ADR-02: Work-Stealing Elastic Concurrency with Sliding-Window Dwell Monitor
* **Context:** High-throughput edge traffic experiences sharp burst spikes. Spawning unbound Tokio tasks can cause thread thrashing and memory exhaustion, while fixed worker thread pools introduce tail-latency queuing.
* **Decision:** Implement an elastic work-stealing pool ([`proxy/src/concurrency/pool.rs`](file:///home/peni/Projects/bridge/proxy/src/concurrency/pool.rs)) using `crossbeam-deque`. Worker count starts at 1 (baseline footprint). A sliding-window queue dwell monitor ([`proxy/src/concurrency/monitor.rs`](file:///home/peni/Projects/bridge/proxy/src/concurrency/monitor.rs)) samples queuing delay; when average dwell exceeds 5ms, dynamic workers spawn up to `max_concurrency` ceiling. Idle workers exit after a 15-second cooldown.
* **Evidence:** Tested in [`tests/concurrency_test.rs`](file:///home/peni/Projects/bridge/tests/concurrency_test.rs) and demonstrated in [`src/bin/demo_autoscaling.rs`](file:///home/peni/Projects/bridge/src/bin/demo_autoscaling.rs).

### ADR-03: Bully Leader Election with Dynamic Quorum Guard
* **Context:** Bridge coordinates ingress entrypoint failover (Cloudflare Tunnel, DNS). In partitioned networks, multiple nodes declaring leadership would result in split-brain DNS thrashing or conflicting tunnel sessions.
* **Decision:** Implement the Bully election algorithm with monotonic terms and rank tuples `(priority, node_id)`. Supplement Bully with an explicit quorum requirement ([`cluster/src/election.rs:57-68`](file:///home/peni/Projects/bridge/cluster/src/election.rs#L57-L68)): candidate promotion requires unique `CoordinatorAck` packets forming a strict majority `(total_known_nodes / 2) + 1`.
* **Evidence:** [`cluster/src/controller.rs:564-725`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L564-L725).

### ADR-04: Lock-Free Read Routing Table via Copy-on-Write ArcSwap
* **Context:** Ingress requests perform domain-to-node route lookups on every connection. Lock contention on standard `RwLock` degrades throughput under concurrent load.
* **Decision:** Use `arc_swap::ArcSwap<HashMap<String, Route>>` for the `DomainRegistry` ([`registry/src/lib.rs:321-324`](file:///home/peni/Projects/bridge/registry/src/lib.rs#L321-L324)). Lookups on the hot path are completely lock-free and wait-free. Route modifications (config reload, Docker discovery events, gossip sync) atomically clone and swap the pointer.
* **Evidence:** [`registry/src/lib.rs:407-444`](file:///home/peni/Projects/bridge/registry/src/lib.rs#L407-L444).

### ADR-05: Push-Pull State Synchronization via Lamport Vector Clocks
* **Context:** In a decentralized mesh where nodes independently discover local containers and receive route updates, gossip dissemination must avoid infinite update loops and resolve concurrent route modifications deterministically.
* **Decision:** Attach `RouteClock { origin_node, version }` to every route ([`registry/src/lib.rs:133-160`](file:///home/peni/Projects/bridge/registry/src/lib.rs#L133-L160)). Nodes periodically exchange compact hash digests (`RouteDigest`) every 500ms. If a remote clock is higher, the local node pulls missing routes; if lower, it pushes updates.
* **Evidence:** [`cluster/src/controller.rs:905-944`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L905-L944), [`cluster/src/controller.rs:1088-1142`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L1088-L1142).

### ADR-06: Leader-Scoped Dashboard Public Ingress
* **Context:** When a cluster has an elected leader managing public ingress, accessing the web dashboard via a public domain must route exclusively to the current leader without leaking local node endpoints across gossip.
* **Decision:** Mark the dashboard public domain as a local-only domain ([`cluster/src/controller.rs:178-180`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L178-L180)). It is filtered out from gossip digests and syncs. On leadership changes, the reactive watcher on the new leader dynamically inserts the route locally, while followers purge it.
* **Evidence:** [`src/daemon.rs:249-271`](file:///home/peni/Projects/bridge/src/daemon.rs#L249-L271), [`src/daemon.rs:377-392`](file:///home/peni/Projects/bridge/src/daemon.rs#L377-L392).

---

## 2. Implementation Ambiguities & Code Unclear Behavior

### AMB-01: Port Sharing Conflict between Ingress and Local Services in Direct Mode
* **Location:** [`proxy/src/core/config.rs:464-474`](file:///home/peni/Projects/bridge/proxy/src/core/config.rs#L464-L474) and [`src/daemon.rs:464-474`](file:///home/peni/Projects/bridge/src/daemon.rs#L464-L474).
* **Observation:** In Direct mode, if a local service is configured on `0.0.0.0:80` and Bridge also binds its HTTP listener on `0.0.0.0:80`, binding fails with `EADDRINUSE`. The code detects when UDP services target their own listen port, but does not prevent TCP port collision between local upstream services and proxy listeners when `node_id = "self"`.
* **Impact:** Operators must ensure backend Docker containers bind non-standard host ports (e.g., `3000`, `8080`) when Bridge binds ports `80` and `443`.

### AMB-02: Failure Detection Cooldown vs. Fast Flapping Nodes
* **Location:** [`cluster/src/controller.rs:226-231`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L226-L231).
* **Observation:** Foca SWIM purges down members after 2 seconds (`remove_down_after = Some(Duration::from_secs(2))`). When a node restarts and rejoins within this 2-second window with a reset incarnation number (`incarnation = 0`), the remaining nodes may temporarily drop its initial `JoinRequest` until the previous incarnation expires from Foca's membership table.
* **Impact:** Mitigated by the periodic rejoin ticker ([`cluster/src/controller.rs:1310-1329`](file:///home/peni/Projects/bridge/cluster/src/controller.rs#L1310-L1329)) which re-announces known peers every 3 seconds, but can introduce up to a 3-second convergence delay on immediate restarts.

---

## 3. Areas with Missing Runtime Evidence

### UNCERTAIN-01: WireGuard Kernel Routing in Unprivileged Containerized Environments
* **Component:** `mesh::WireGuardDevice::sync_to_kernel` ([`mesh/src/wireguard.rs:151-220`](file:///home/peni/Projects/bridge/mesh/src/wireguard.rs#L151-L220)).
* **Limitation:** In unit test environments and unprivileged Docker containers lacking `CAP_NET_ADMIN`, calls to `WGApi::new()` or `wgapi.create_interface()` fail. The code catches these errors and logs warnings, returning `Ok(())`.
* **Runtime Verification:** Kernel WireGuard interface creation and Netlink routing can only be verified on a host system running Linux with root privileges or `CAP_NET_ADMIN`. Static code and unit tests verify in-memory peer map management, but do not execute live kernel netlink calls during standard CI.

### UNCERTAIN-02: Cloudflare API Rate Limiting during Frequent Leadership Elections
* **Component:** `src/handoff/dns.rs` ([`DnsManager::sync_dns`](file:///home/peni/Projects/bridge/src/handoff/dns.rs#L138-L235)).
* **Limitation:** If a cluster experiences frequent leadership flaps (e.g. priority ties or network packet loss), `DnsManager` executes Cloudflare API requests on every election. The current implementation does not implement client-side exponential backoff or rate-limiting for Cloudflare DNS API calls.
* **Runtime Verification:** Verified using mock HTTP API server in test suite; live Cloudflare API behavior under high flap rates is unverified.

---

## 4. Architectural Recommendations (Proposed Decisions)

* **REC-01 (Documentation Update):** Officially align documentation with the codebase: update `README.md` to state 360 virtual nodes, remove references to `instant-acme`, and clarify that L4 streaming uses Tokio userspace `copy_bidirectional`.
* **REC-02 (Floating IP Implementation or Deprecation):** Either implement cloud provider API drivers (Hetzner, DigitalOcean) for `HandoffMode::FloatingIp`, or formally deprecate the enum variant to eliminate confusion.
* **REC-03 (DNS API Flap Damping):** Introduce a minimum hold-down timer (e.g. 15 seconds) in `DnsManager` before updating DNS records, preventing DNS cache thrashing during transient cluster elections.
