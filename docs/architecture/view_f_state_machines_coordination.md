# View F: State Machines and Distributed Coordination Architecture

## 1. Overview

This view documents the state machines governing node lifecycle, failure detection, Bully leader election, and distributed routing table synchronization.

---

## 2. Cluster Node Lifecycle & Failure Detection (Foca SWIM State Machine)

Every node in the cluster is tracked by the Foca SWIM epidemic failure detector through internal membership transitions.

```mermaid
stateDiagram-v2
    [*] --> Unknown: Initial Boot / Uncontacted

    Unknown --> Alive: JoinRequest / Announce Received
    Alive --> Suspect: Direct Ping Timeout (150ms probe_rtt)\nIndirect PingReq Failed
    
    Suspect --> Alive: Direct/Indirect Probe Ack Received\n(Higher Incarnation Number)
    Suspect --> Down: suspect_to_down_after (1000ms) Expired
    
    Down --> Removed: remove_down_after (2000ms) Expired
    Down --> Alive: Rejoin / Announce Received with Renewed Incarnation
    
    Removed --> Alive: Periodic Rejoin Ticker / New JoinRequest

    note right of Alive
        MemberEvent::Up emitted.
        Peer added to WireGuard wg0 device.
        AllowedIPs route programmed.
    end note

    note right of Down
        MemberEvent::Down emitted.
        Peer evicted from WireGuard wg0.
        Routes targeting peer pruned.
        If Dead Member was Leader: Trigger Bully Election.
        If Replicated Workload on Member: Trigger Failover.
    end note
```

### Invariants & Guards
* **Incarnation Monotonicity:** When a node recovers after being suspected or marked down, it bumps its incarnation number (`incarnation = incarnation + 1`). Incoming gossip messages with lower incarnation numbers are ignored, preventing stale "Down" rumors from poisoning recovered nodes.
* **Rapid Eviction:** Down members are purged from Foca's internal active list after 2 seconds, allowing restarted VMs to quickly rejoin without being rejected as dead.

---

## 3. Bully Leader Election State Machine

The leader election state machine guarantees that across any non-partitioned cluster of $N$ nodes, exactly one node with verified majority quorum acts as cluster coordinator.

```mermaid
stateDiagram-v2
    [*] --> Follower: Node Initialized (Term 0, Leader None)

    Follower --> Candidate: Startup Grace Period (2s) Expired without Leader\nOR Acknowledged Leader Dies (MemberDown)\nOR Higher Node Appears (Challenge)\nOR CLI trigger

    state Candidate {
        [*] --> EvaluatingRank
        EvaluatingRank --> AwaitingElectionOk: Higher-ranked peers exist\nBroadcast Election to higher nodes
        EvaluatingRank --> DeclaringCoordinator: No higher-ranked peers exist\nLocal node is highest rank!

        AwaitingElectionOk --> CandidateRestart: Higher peer sent ElectionOk, but failed\nto declare Coordinator within 600ms
        AwaitingElectionOk --> FollowerYield: Higher peer broadcasts Coordinator

        DeclaringCoordinator --> AwaitingAcks: Broadcast Coordinator to all peers\nReset acks set, Start ack_timeout (300ms)
        AwaitingAcks --> QuorumSuccess: (1 + acks.len()) >= quorum_required\nwhere quorum = (N / 2) + 1
        AwaitingAcks --> QuorumFailed: ack_timeout expired && < quorum
    }

    QuorumSuccess --> Leader: Promoted to Leader\nBroadcast leader_tx watch update
    QuorumFailed --> Follower: Step down to Follower\n(Wait for cluster healing)
    CandidateRestart --> Candidate: Increment Term, Re-evaluate
    FollowerYield --> Follower: Acknowledge higher leader

    Leader --> Follower: Higher priority node joins\nOR Administrative step-down CLI command\nOR Received higher-term Coordinator
```

### Quorum Formulation & Invariants
$$\text{Quorum Required} = \begin{cases} 
\text{explicit\_override} & \text{if } \text{quorum\_size is set} \\
\left\lfloor \frac{\text{total\_known\_nodes}}{2} \right\rfloor + 1 & \text{otherwise}
\end{cases}$$

* **Single-Node Invariant:** If `total_known_nodes <= 1`, quorum is 1 (self vote). A standalone node assumes leadership trivially without network timeouts.
* **Term Monotonicity:** A node rejects any `Election` or `Coordinator` message whose term is strictly less than its current term.
* **Self-Healing Loop:** Every 5 seconds, if known peers exist but `current_leader == None` (e.g. following a split election), the controller automatically re-runs `start_election()`.

---

## 4. Ingress Handoff Leadership State Machine

Ingress entrypoints (Cloudflare Tunnel and Cloudflare DNS) are strictly bound to cluster leadership state via Tokio `watch::Receiver<Option<PeerNode>>`.

```mermaid
stateDiagram-v2
    [*] --> Standby: Startup

    state Standby {
        ColdStandby: cloudflared process stopped / DNS unchanged
        WarmStandby: cloudflared running continuously on all nodes
    }

    Standby --> PromotedLeader: leader_rx notifies Local Node is Leader

    state PromotedLeader {
        StartTunnel: Spawn child process `cloudflared tunnel run`
        SyncDns: Call Cloudflare API PATCH /zones/{id}/dns_records/{id}\nPoint A-record to local IP
        ActivateDashboardRoute: Insert public dashboard domain into local DomainRegistry
    }

    PromotedLeader --> Standby: leader_rx notifies Local Node is NOT Leader\n(Step down / lost election)

    state StepDownActions {
        KillTunnel: Terminate `cloudflared` child process (SIGKILL / timeout)
        RemoveDashboardRoute: DomainRegistry::remove(public_domain)
    }
```

---

## 5. Workload Replication & Failback State Machine

```mermaid
stateDiagram-v2
    [*] --> Idle: Service Running on Original VM

    Idle --> FailoverTriggered: MemberDown received for service host VM\n(Local node is elected leader)

    state FailoverTriggered {
        SelectPlacement: Consistent hash successor ("ring") / "leader" / explicit
        SpawnContainer: DockerContainerDriver::spawn_container(...)
        UpdateRoute: DomainRegistry route redirected to failover instance
        BroadcastRoute: RouteSync datagram sent to all peers
    }

    FailoverTriggered --> ActiveReplica: Container Running & Serving Ingress Traffic

    ActiveReplica --> MemberRecovered: MemberUp received for original VM

    state MemberRecovered {
        CheckMode: Evaluate failback_mode setting
    }

    CheckMode --> ActiveReplica: NonPreemptive: Do nothing\nKeep replica active
    CheckMode --> CooldownTimer: Preemptive: Start failback cooldown timer (10s)
    CheckMode --> AwaitManual: Manual: Wait for operator CLI command

    CooldownTimer --> RestoreOriginal: Cooldown expired without failure
    AwaitManual --> RestoreOriginal: Operator runs `bridge failback <domain>`

    state RestoreOriginal {
        StopContainer: DockerContainerDriver::stop_container(...)
        RevertRoute: DomainRegistry route reset to original host
        BroadcastRevert: RouteSync datagram sent to all peers
    }

    RestoreOriginal --> Idle: Workload Returned to Original Host
```
