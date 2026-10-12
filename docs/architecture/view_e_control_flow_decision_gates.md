# View E: Control Flow and Decision Gates Architecture

## 1. Overview

This view details the major decision gates, policy branching, guard evaluations, and sequence interactions executed by BRIDGE during connection handling, leader election, and workload failover.

---

## 2. Ingress Connection Dispatch & Routing Decision Flow

```mermaid
flowchart TD
    StartConn(["Incoming TCP Connection Accepted"]) --> CheckMode{"proxy.config.mode"}

    %% Managed Mode Guard
    CheckMode -->|"Managed Mode"| ErrManaged["Managed Mode does not bind TCP listeners\n(Traefik handles traffic directly)"]

    %% Direct Mode Branch
    CheckMode -->|"Direct Mode"| HyperRead["Hyper HTTP Parser reads request"]
    HyperRead --> HostExtract{"Host Header present?"}
    HostExtract -->|"No"| DirectNoHost["Return 200 'No host found in request uri'"]
    HostExtract -->|"Yes"| NormalizeHost["normalize_host(host)\nStrip port & lowercase"]
    NormalizeHost --> LookupDirect{"DomainRegistry::lookup(host)"}
    LookupDirect -->|"None"| DirectNotFound["Return 200 'No route found for host'"]
    LookupDirect -->|"Some(route)"| UpstreamCheck{"route.upstream is Some?"}
    UpstreamCheck -->|"None"| DirectNoUpstream["Return 200 'No upstream found for host'"]
    UpstreamCheck -->|"Some(addr)"| MethodCheck{"req.method == CONNECT?"}
    MethodCheck -->|"Yes"| HyperTunnel["Upgrade connection & tunnel TCP stream"]
    MethodCheck -->|"No"| HyperProxy["Proxy request via Hyper Client connection pool"]

    %% Handoff Mode Branch
    CheckMode -->|"Handoff Mode"| ReadBuffer["Read first 4096 bytes into buffer"]
    ReadBuffer --> ReadBytes{"bytes_read == 0?"}
    ReadBytes -->|"Yes (EOF)"| CloseClient["Close Connection immediately"]
    ReadBytes -->|"No"| CheckTlsRecord{"buf[0] == 0x16 && len >= 5?"}
    CheckTlsRecord -->|"Yes (TLS Record)"| PeekClientHello["Read remainder of ClientHello record\n(Timeout: 500ms, Max: 16384 bytes)"]
    CheckTlsRecord -->|"No (Plaintext)"| ParseSni
    PeekClientHello --> ParseSni{"parse_tls_sni(&buf)"}

    ParseSni -->|"Some(domain)"| LookupHandoff
    ParseSni -->|"None"| ParseHttpHost{"parse_http_host(&buf)"}

    ParseHttpHost -->|"Some(host)"| CheckRedirect{"proxy.config.redirect_http?"}
    ParseHttpHost -->|"None"| DropUnroutable["Drop/Close connection (Unable to extract SNI or Host)"]

    CheckRedirect -->|"true"| ParseUri["parse_http_uri_path(&buf)"]
    ParseUri --> Send301["Send HTTP/1.1 301 Moved Permanently\nLocation: https://{host}{uri}"]
    Send301 --> CloseClient
    CheckRedirect -->|"false"| LookupHandoff

    LookupHandoff{"DomainRegistry::lookup(domain) || lookup(trimmed_dot)"}
    LookupHandoff -->|"None"| CheckIsTls{"is_tls == true?"}
    CheckIsTls -->|"true"| SilentClose["Close connection silently (avoid TLS handshake error)"]
    CheckIsTls -->|"false"| SendPlain404["Send HTTP 200 'No route found for host {domain}'"]
    SendPlain404 --> CloseClient

    LookupHandoff -->|"Some(route)"| TargetSelection{"route.targets.len() > 1?"}
    TargetSelection -->|"Yes (Multi-node ring)"| RingSelect["route.ring.get_for_client(client_ip, domain)\nConsistent hash: SHA256(ClientIP + domain)"]
    TargetSelection -->|"No (Single node)"| PrimarySelect["Primary route.node"]

    RingSelect --> TargetAddrCalc["target_node.target_address_with_fallback(mode.routing)\nDirect (public IP) vs Mesh (10.8.0.x WireGuard)"]
    PrimarySelect --> TargetAddrCalc

    TargetAddrCalc --> CheckProxyProto{"target_node.should_send_proxy_protocol(default)?"}
    CheckProxyProto -->|"true"| EncodePpv2["encode_proxy_v2(client_addr, local_addr)\nPrepend 16-byte binary header"]
    CheckProxyProto -->|"false"| ConnectBackend["TcpStream::connect(target_addr)"]
    EncodePpv2 --> ConnectBackend

    ConnectBackend --> ConnResult{"Connection succeeded?"}
    ConnResult -->|"Err(e)"| LogErr["Log error & return std::io::Error"]
    ConnResult -->|"Ok(target_stream)"| SendInitialBuf["target_stream.write_all(&buf)"]
    SendInitialBuf --> SpliceStreams["tokio::io::copy_bidirectional(&mut client, &mut target)"]
    SpliceStreams --> CloseClient
```

---

## 3. Bully Leader Election Sequence

This sequence diagram depicts a node initiating an election round, encountering higher-ranked peers, handling unresponsive nodes, and declaring coordinator leadership upon verified majority quorum.

```mermaid
sequenceDiagram
    autonumber
    actor Trigger as Startup / Leader Death
    participant Node2 as Node-02 (Priority 100)
    participant Node3 as Node-03 (Priority 200)
    participant Node1 as Node-01 (Priority 50)
    participant WireGuard as Mesh Peer Interface

    Trigger->>Node2: Failure detected or startup grace period expires
    Note over Node2: start_election()<br/>term = term + 1 (term 1)<br/>role = Candidate<br/>total_known_nodes = 3<br/>quorum_required = (3 / 2) + 1 = 2

    Node2->>Node2: Find peers with rank > my_rank (Node-03 has priority 200)
    Node2->>Node3: Raw UDP: ClusterMessage::Election { from_node: "node-02", term: 1, priority: 100 }
    Note over Node2: Sleep election_timeout (300ms) awaiting ElectionOk

    alt Node-03 is Alive
        Node3-->>Node2: Raw UDP: ClusterMessage::ElectionOk { from_node: "node-03", term: 1 }
        Note over Node2: got_election_ok = true<br/>Yield to higher-ranked node
        Node3->>Node3: start_election()<br/>term = 2, role = Candidate
        Node3->>Node3: Higher peers check: None (Node-03 is highest!)
        Node3->>Node2: Raw UDP: ClusterMessage::Coordinator { leader: Node-03, term: 2 }
        Node3->>Node1: Raw UDP: ClusterMessage::Coordinator { leader: Node-03, term: 2 }
        Node2-->>Node3: Raw UDP: ClusterMessage::CoordinatorAck { from_node: "node-02", term: 2 }
        Node1-->>Node3: Raw UDP: ClusterMessage::CoordinatorAck { from_node: "node-01", term: 2 }
        Note over Node3: acks count = 2 (self vote + 2 = 3 >= quorum 2)<br/>Quorum Verified!<br/>role = Leader<br/>Broadcast leader_tx watch update
    else Node-03 is Dead / Unresponsive
        Note over Node2: Timeout (300ms) expires with no ElectionOk!<br/>Higher peer unresponsive: Node-02 steps up!
        Node2->>Node2: declare_coordinator(term 1)
        Node2->>Node1: Raw UDP: ClusterMessage::Coordinator { leader: Node-02, term: 1 }
        Node1-->>Node2: Raw UDP: ClusterMessage::CoordinatorAck { from_node: "node-01", term: 1 }
        Note over Node2: acks count = 1 (self vote + 1 = 2 >= quorum 2)<br/>Quorum Verified!<br/>role = Leader<br/>Broadcast leader_tx watch update
    end
```

---

## 4. Leader-Orchestrated Workload Failover & Failback Sequence

```mermaid
sequenceDiagram
    autonumber
    participant SWIM as Foca SWIM Failure Detector
    participant Controller as ClusterController (Leader)
    participant Duplicator as WorkloadDuplicator
    participant Driver as DockerContainerDriver
    participant PeerNode as Surviving Peer VM (Node-03)
    participant Registry as DomainRegistry

    SWIM->>Controller: OwnedNotification::MemberDown("vm-02")
    Controller->>Duplicator: broadcast::send(MemberEvent::Down("vm-02"))

    Note over Duplicator: Check: Is local node the Leader?<br/>YES (Leader coordinates failover)
    Duplicator->>Duplicator: Find replicated services for "vm-02"<br/>Found: api.example.com (image: my-api:latest)

    alt Placement == Local Leader or Ring Successor is Local
        Duplicator->>Driver: spawn_container("api.example.com", "my-api:latest", env, port: 8080)
        Driver-->>Duplicator: Ok(SocketAddr: 127.0.0.1:8080)
        Duplicator->>Registry: Update route "api.example.com" -> 127.0.0.1:8080
        Duplicator->>Controller: broadcast_route("api.example.com", Route)
    else Placement == Remote Peer ("vm-03")
        Duplicator->>Controller: send_spawn_workload("vm-03", domain, image, env, port)
        Controller->>PeerNode: Raw UDP: ClusterMessage::SpawnWorkload
        PeerNode->>PeerNode: Spawn Docker container locally
        PeerNode-->>Controller: Raw UDP: ClusterMessage::WorkloadSpawned { target_addr: 10.8.0.3:8080 }
        Controller->>Duplicator: WorkloadCommand::Spawned
        Duplicator->>Registry: Update route "api.example.com" -> 10.8.0.3:8080 (mesh)
        Duplicator->>Controller: broadcast_route("api.example.com", Route)
    end

    Note over Controller,Duplicator: --- RECOVERY / FAILBACK CYCLE ---
    SWIM->>Controller: OwnedNotification::MemberUp("vm-02")
    Controller->>Duplicator: broadcast::send(MemberEvent::Up("vm-02"))

    alt FailbackMode == NonPreemptive
        Note over Duplicator: NonPreemptive: Do nothing.<br/>Leave replica running to avoid breaking sessions.
    else FailbackMode == Preemptive
        Note over Duplicator: Preemptive: Start cooldown timer (failback_cooldown_secs = 10s)
        Duplicator->>Duplicator: Sleep 10s (Verify node stability)
        Duplicator->>Driver: stop_container("api.example.com")
        Duplicator->>Registry: Restore route "api.example.com" -> vm-02 original endpoint
        Duplicator->>Controller: broadcast_route("api.example.com", Route)
    else FailbackMode == Manual
        Note over Duplicator: Manual: Wait until operator runs:<br/>bridge failback api.example.com
    end
```
