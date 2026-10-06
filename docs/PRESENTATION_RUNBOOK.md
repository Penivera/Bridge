# Bridge — Presentation & Live Demo Runbook

This guide provides a step-by-step walkthrough for presenting the **Bridge** architecture, demonstrating its distributed features, and running the live terminal and dashboard demos.

---

## 1. Executive Summary & Talking Points

* **What Bridge Solves:**  
  Bridge is a zero-dependency, decentralized edge mesh proxy and service orchestrator designed for multi-VPS fleets (e.g. Coolify, Traefik, Docker). It replaces single-point-of-failure load balancers with an embedded, self-healing peer-to-peer mesh.
* **Core Technological Pillars:**
  1. **Transparent Ingress Handoff:** SNI-based TCP stream splicing without TLS termination (preserving upstream certificates and zero decrypt overhead).
  2. **Automated WireGuard Overlay Mesh:** Kernel/userspace peer mesh with raw UDP gossip synchronization.
  3. **SWIM Gossip Failure Detection:** Sub-second failure detection via embedded `foca`.
  4. **Bully Leader Election & Dynamic Quorum:** Split-brain protection with majority quorum verification ($\lceil n/2 \rceil$).
  5. **Consistent Hashing with 360 Virtual Nodes:** Deterministic session affinity (`SHA256(source_ip + hostname)`) with bounded $\pm 10\text{--}15\%$ uniform distribution.
  6. **Leader-Orchestrated Workload Duplication (Approach A):** Docker container spawning on surviving nodes upon node failure with configurable failback modes (*Preemptive*, *NonPreemptive*, *Manual*).
  7. **Fleet Observability & Control:** Unix Domain Socket IPC CLI and an embedded real-time web dashboard.

---

## 2. Pre-Presentation Setup (1 Minute)

Ensure the binary compiles cleanly:
```bash
cargo build --release
```

The sample presentation configuration is ready at [`examples/demo.toml`](file:///home/peni/Projects/bridge/examples/demo.toml).

---

## 3. Step-by-Step Live Demo Script

### Step 3.1 — Start the Bridge Daemon
In **Terminal 1**, start Bridge using the demo configuration:
```bash
cargo run -- --config examples/demo.toml
```
*Expected Output:*
```text
INFO bridge: IPC control server listening socket=/tmp/bridge.sock
INFO bridge: observability dashboard listening at http://127.0.0.1:9090
INFO bridge: bridge is running mode=Handoff routes=3 dashboard=true
```

---

### Step 3.2 — Open the Observability Web Dashboard
In your browser, open:
```text
http://127.0.0.1:9090
```
* **Visual Highlights to Point Out:**
  * Top status pills showing **Local Node ID (`vm-01`)**, **Cluster Leader badge (`LEADER`)**, active routes, and zero replicas.
  * **Cluster Nodes & WireGuard Mesh Table** showing physical endpoints and overlay `10.8.0.x` IPs.
  * **Active Routes Table** highlighting `app.example.com` with its **`RING (360 vnodes)`** badge.
  * **Demo Action Bar** with one-click buttons for failure simulation and failback.

---

### Step 3.3 — Terminal Demo: Fleet Inspection (Terminal 2)

Open **Terminal 2** and inspect the running cluster:

1. **Check Daemon Status:**
   ```bash
   target/debug/bridge status
   ```

2. **Inspect Cluster Membership & Leader State:**
   ```bash
   target/debug/bridge cluster
   ```
   *Shows local node role, cluster coordinator, and all peer nodes.*

3. **Inspect Consistent Hashing & Session Affinity:**
   ```bash
   target/debug/bridge inspect app.example.com --client-ip 192.168.1.100
   ```
   *Demonstrates how Bridge maps client `192.168.1.100` deterministically to a specific backend using composite SHA-256 tokens.*

---

### Step 3.4 — Live Demo: Node Failure & Workload Duplication

Now simulate node failure on `vm-02` live:

1. **Trigger Failover Duplication via CLI:**
   ```bash
   target/debug/bridge replicate --node-id vm-02
   ```
   *Output:*
   ```text
   [DEMO] Triggering failover duplication for node 'vm-02'...
   SUCCESS: Spawned 1 duplicate service(s) for node 'vm-02'
   Traffic now routing to failover instances.
   ```

2. **Verify Active Replicas in Terminal:**
   ```bash
   target/debug/bridge replicas
   ```
   *Shows `api.example.com` duplicated from origin `vm-02` onto leader `vm-01` running on port `127.0.0.1:18000` with Preemptive cooldown timer.*

3. **Show the Web Dashboard:**
   * Point to the browser — the **Active Failover Replicas** table automatically updated in real-time to show the running container replica!

---

### Step 3.5 — Live Demo: Failback Recovery

Restore the service back to the primary node:

1. **Execute Failback via CLI (or click the button on the Web Dashboard):**
   ```bash
   target/debug/bridge failback api.example.com
   ```
   *Output:*
   ```text
   Triggering failback for domain 'api.example.com'...
   SUCCESS: Failback executed. Service 'api.example.com' restored to origin node.
   ```

2. **Verify Replicas Cleared:**
   ```bash
   target/debug/bridge replicas
   ```
   *Output: `No active service replicas.`*
   *The browser dashboard immediately updates, clearing the replica table and showing all routes normal.*

---

## 4. Test Suite Verification (If Asked About Code Quality)

Run the automated test suite during Q&A:
```bash
# Run all 20 test modules (over 125 tests)
cargo test

# Or run specific feature tests:
cargo test --test cli_and_dashboard_test
cargo test --test service_duplication_test
cargo test --test consistent_hashing_test
```
