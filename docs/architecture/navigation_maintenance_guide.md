# Navigation and Maintenance Guide

This guide explains how to view, navigate, verify, and maintain the BRIDGE architecture documentation set and its associated diagrams.

---

## 1. How to Navigate the Architecture Documentation

The documentation is organized hierarchically, allowing readers to start at the top-level system context and drill down into subsystem mechanics:

```
docs/architecture/
├── README.md                              <-- Master Architecture Overview & Entry Index
├── unified_system_architecture.md         <-- View K: Unified Full-System Architecture Map
├── unified_architecture.svg               <-- Rendered Zoomable Vector Map (81 KB SVG)
├── unified_architecture.dot               <-- Graphviz Source for Unified Architecture Map
│
├── view_a_system_context.md               <-- View A: System Boundary, Actors & Cloud Services
├── view_b_container_deployment.md         <-- View B: Systemd Daemon, Process Topology & Network
├── view_c_component_architecture.md       <-- View C: Rust Crates & Module Decomposition
├── view_d_data_flow.md                    <-- View D: Data Paths (Handoff, Direct, Managed, UDP)
├── view_e_control_flow_decision_gates.md  <-- View E: Decision Trees & Interaction Sequences
├── view_f_state_machines_coordination.md  <-- View F: Foca SWIM & Bully Election State Machines
├── view_g_persistence_architecture.md     <-- View G: In-Memory Stores, bridge-state.json & TOML
├── view_h_api_integration.md              <-- View H: IPC Protocol, Dashboard REST/WS & Docker API
├── view_i_security_trust_boundaries.md    <-- View I: WireGuard Cryptography, Capabilities & Auth
├── view_j_operations_failure.md           <-- View J: CI/CD, Packaging, Health Checks & Failure
│
├── evidence_register.md                   <-- Traceability Register: Code Symbols to Claims
├── documentation_reconciliation.md        <-- Discrepancy Register & Reconciliation Report
├── decision_uncertainty_register.md       <-- Architectural Decisions, Ambiguities & Trade-offs
└── navigation_maintenance_guide.md        <-- This Maintenance & Regeneration Manual
```

### Viewing Options
1. **Markdown Viewers (GitHub / IDEs / Web):** All `.md` documents contain embedded standard **Mermaid** diagrams that render natively in GitHub, GitLab, VS Code, and browser markdown previewers.
2. **Interactive Vector View (Browser / Inkscape):** Open [`unified_architecture.svg`](unified_architecture.svg) directly in Google Chrome, Firefox, or an image viewer for infinite zoom and pan inspection across the full-system map.

---

## 2. Tools & Dependencies for Diagram Regeneration

The architecture diagrams are generated from plain text source files without heavyweight proprietary software:

* **Mermaid:** Built into GitHub Flavored Markdown and standard documentation viewers. No compilation needed for web reading.
* **Graphviz (`dot`):** Used to compile the vector SVG architecture map.
  * *Package on Ubuntu/Debian:* `sudo apt-get install -y graphviz`
  * *Version Verified:* `dot - graphviz version 14.1.2`
* **Rust & Cargo:** Used to run the test suite and verify code symbols against architectural claims:
  * *Rust Toolchain:* 1.80+ (2024 edition)

---

## 3. How to Regenerate or Update the Architecture Diagrams

### 3.1 Regenerating the Unified Vector Diagram (`unified_architecture.svg`)
When structural changes, new modules, or new external integration points are added:

1. Edit [`docs/architecture/unified_architecture.dot`](unified_architecture.dot).
2. Run the Graphviz compiler from the project root:
   ```bash
   dot -Tsvg docs/architecture/unified_architecture.dot -o docs/architecture/unified_architecture.svg
   ```
3. Update the corresponding Mermaid flowchart in [`docs/architecture/unified_system_architecture.md`](unified_system_architecture.md).

### 3.2 Updating Sequence or Component Diagrams
All subsystem views (Views A through J) use standard Mermaid fenced code blocks (` ```mermaid `). To update:
1. Locate the relevant subsystem file (e.g., `view_e_control_flow_decision_gates.md` for sequence changes).
2. Modify the Mermaid source directly in the markdown file.
3. Verify syntax rendering using an IDE previewer or the [Mermaid Live Editor](https://mermaid.live).

---

## 4. Source of Truth Guidelines

To maintain documentation integrity across the codebase lifecycle:

1. **The Codebase is the Final Source of Truth:** If an existing document claims functionality that does not exist in code (e.g. `instant-acme` TLS termination or `splice(2)` kernel syscalls), the implementation in `src/`, `proxy/`, `cluster/`, `mesh/`, and `registry/` takes precedence.
2. **Synchronize Code Changes with the Evidence Register:** When adding a new subsystem or modifying protocol behavior, update [`docs/architecture/evidence_register.md`](evidence_register.md) with the new file paths, struct names, and line ranges.
3. **Keep Checkpoint Schemas Updated:** If `RuntimeState` in [`proxy/src/shutdown.rs`](file:///home/peni/Projects/bridge/proxy/src/shutdown.rs) changes, update the JSON schema in [`view_g_persistence_architecture.md`](view_g_persistence_architecture.md).
4. **Preserve Wire Protocol Stability:** Changes to `ClusterMessage` in [`cluster/src/protocol.rs`](file:///home/peni/Projects/bridge/cluster/src/protocol.rs) constitute wire-breaking protocol modifications. Increment `PROTOCOL_VERSION` (currently `1`) and update [`view_h_api_integration.md`](view_h_api_integration.md).
