use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bridge::cluster::{ClusterController, ClusterMessage, PeerNode};
use bridge::mesh::WireGuardDevice;
use proxy::ShutdownReason;
use registry::{DomainRegistry, Node, Route};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, RwLock};

async fn create_test_node(
    node_id: &str,
    mesh_ip_last_octet: u8,
) -> (Arc<ClusterController>, SocketAddr, broadcast::Sender<ShutdownReason>) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local_addr = socket.local_addr().unwrap();

    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, mesh_ip_last_octet));
    let peer_node = PeerNode::new(
        node_id,
        format!("pubkey_{node_id}"),
        mesh_ip,
        local_addr,
    );

    let wg = Arc::new(RwLock::new(WireGuardDevice::new(
        "wg0",
        format!("privkey_{node_id}"),
        format!("pubkey_{node_id}"),
        local_addr.port(),
        mesh_ip,
    )));

    let registry = Arc::new(DomainRegistry::new());
    let controller = Arc::new(ClusterController::new_with_socket(peer_node, socket, wg, registry));

    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let ctrl_clone = controller.clone();
    tokio::spawn(async move {
        let _ = ctrl_clone.run_with_shutdown(shutdown_rx, None).await;
    });

    (controller, local_addr, shutdown_tx)
}

#[tokio::test]
async fn test_two_node_udp_bootstrap_and_wireguard_sync() {
    // 1. Start Seed Node S
    let (seed_ctrl, seed_addr, seed_shutdown) = create_test_node("seed-node", 1).await;

    // 2. Start Joining Node N
    let (node_ctrl, _node_addr, node_shutdown) = create_test_node("node-joiner", 2).await;

    // 3. Node N bootstraps via UDP to Seed S
    let peers_synced = node_ctrl
        .bootstrap(&[seed_addr])
        .await
        .expect("bootstrap over UDP");

    assert_eq!(peers_synced, 1, "node should have synced 1 peer from seed");

    // Allow background packet processing to settle
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. Verify Node N state
    {
        let peers = node_ctrl.peers.read().await;
        assert!(peers.contains_key("seed-node"));
        let wg = node_ctrl.wireguard.read().await;
        assert_eq!(wg.peer_count(), 1);
        assert!(wg.get_peer("pubkey_seed-node").is_some());
        assert!(node_ctrl.registry.lookup("seed-node.node.internal").is_some());
    }

    // 5. Verify Seed S state (learned Node N via JoinRequest)
    {
        let peers = seed_ctrl.peers.read().await;
        assert!(peers.contains_key("node-joiner"));
        let wg = seed_ctrl.wireguard.read().await;
        assert_eq!(wg.peer_count(), 1);
        assert!(wg.get_peer("pubkey_node-joiner").is_some());
        assert!(seed_ctrl.registry.lookup("node-joiner.node.internal").is_some());
    }

    let _ = seed_shutdown.send(ShutdownReason::Manual);
    let _ = node_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_three_node_dynamic_peer_announcement() {
    // 1. Seed S and Node A bootstrap together
    let (seed_ctrl, seed_addr, seed_shutdown) = create_test_node("seed-0", 1).await;
    let (node_a_ctrl, _node_a_addr, node_a_shutdown) = create_test_node("node-a", 2).await;

    node_a_ctrl.bootstrap(&[seed_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Third node (Node B) joins via Seed S
    let (node_b_ctrl, _node_b_addr, node_b_shutdown) = create_test_node("node-b", 3).await;
    let b_peers = node_b_ctrl.bootstrap(&[seed_addr]).await.unwrap();
    assert!(b_peers >= 1);

    // Give time for Seed S to send PeerAnnounce to Node A
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Verify all 3 nodes have each other in WireGuard
    {
        let wg_seed = seed_ctrl.wireguard.read().await;
        assert_eq!(wg_seed.peer_count(), 2); // knows node-a and node-b
    }
    {
        let wg_a = node_a_ctrl.wireguard.read().await;
        assert_eq!(wg_a.peer_count(), 2); // knows seed-0 and node-b (via announce!)
        assert!(wg_a.get_peer("pubkey_node-b").is_some());
    }
    {
        let wg_b = node_b_ctrl.wireguard.read().await;
        assert!(wg_b.peer_count() >= 1); // knows seed-0 and node-a
    }

    let _ = seed_shutdown.send(ShutdownReason::Manual);
    let _ = node_a_shutdown.send(ShutdownReason::Manual);
    let _ = node_b_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_swim_ping_ack_roundtrip() {
    let (node_a, _a_addr, a_shutdown) = create_test_node("node-ping-a", 10).await;
    let (_node_b, b_addr, b_shutdown) = create_test_node("node-ping-b", 11).await;

    // Send direct SWIM Ping from Node A to Node B
    let ping = ClusterMessage::Ping {
        seq: 1001,
        from_node: "node-ping-a".to_string(),
    };
    node_a.socket.send_to(&ping.encode().unwrap(), b_addr).await.unwrap();

    // Node A awaits Ack from Node B
    let mut buf = vec![0u8; 1024];
    let timeout = Duration::from_millis(1000);
    let res = tokio::time::timeout(timeout, async {
        let (len, src) = node_a.socket.recv_from(&mut buf).await.unwrap();
        assert_eq!(src, b_addr);
        ClusterMessage::decode(&buf[..len]).unwrap()
    }).await;

    match res {
        Ok(ClusterMessage::Ack { seq, from_node }) => {
            assert_eq!(seq, 1001);
            assert_eq!(from_node, "node-ping-b");
        }
        other => panic!("expected ClusterMessage::Ack, got {other:?}"),
    }

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_foca_swim_graceful_leave_and_member_down() {
    let (node_a, _a_addr, a_shutdown) = create_test_node("node-swim-a", 20).await;
    let (_node_b, b_addr, b_shutdown) = create_test_node("node-swim-b", 21).await;

    // 1. Node A bootstraps with Node B
    node_a.bootstrap(&[b_addr]).await.unwrap();

    // Settle bootstrap
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify both nodes initially have each other
    {
        let peers_a = node_a.peers.read().await;
        assert!(peers_a.contains_key("node-swim-b"), "node A should know node B");
        let wg_a = node_a.wireguard.read().await;
        assert_eq!(wg_a.peer_count(), 1);
    }

    // 2. Node B shuts down gracefully, broadcasting SWIM leave tombstone
    let _ = b_shutdown.send(ShutdownReason::Sigterm);

    // Wait for SWIM leave datagram to be processed by Node A
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. Verify Node A processed MemberDown and auto-pruned Node B from mesh
    {
        let peers_a = node_a.peers.read().await;
        assert!(!peers_a.contains_key("node-swim-b"), "node A should have unlearned node B after leave");
        let wg_a = node_a.wireguard.read().await;
        assert_eq!(wg_a.peer_count(), 0, "WireGuard peer should be removed upon SWIM leave");
        assert!(node_a.registry.lookup("node-swim-b.node.internal").is_none(), "Domain route should be pruned");
    }

    let _ = a_shutdown.send(ShutdownReason::Manual);
}

async fn create_fast_test_node(
    node_id: &str,
    mesh_ip_last_octet: u8,
) -> (
    Arc<ClusterController>,
    SocketAddr,
    broadcast::Sender<ShutdownReason>,
    tokio::task::JoinHandle<()>,
) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local_addr = socket.local_addr().unwrap();

    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, mesh_ip_last_octet));
    let peer_node = PeerNode::new(
        node_id,
        format!("pubkey_{node_id}"),
        mesh_ip,
        local_addr,
    );

    let wg = Arc::new(RwLock::new(WireGuardDevice::new(
        "wg0",
        format!("privkey_{node_id}"),
        format!("pubkey_{node_id}"),
        local_addr.port(),
        mesh_ip,
    )));

    let mut config = foca::Config::simple();
    config.probe_period = Duration::from_millis(60);
    config.probe_rtt = Duration::from_millis(20);
    config.suspect_to_down_after = Duration::from_millis(150);

    let registry = Arc::new(DomainRegistry::new());
    let controller = Arc::new(ClusterController::new_with_foca_config(
        peer_node, socket, wg, registry, config,
    ));

    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let ctrl_clone = controller.clone();
    let handle = tokio::spawn(async move {
        let _ = ctrl_clone.run_with_shutdown(shutdown_rx, None).await;
    });

    (controller, local_addr, shutdown_tx, handle)
}

#[tokio::test]
async fn test_foca_swim_ungraceful_kill_and_recovery() {
    let (node_a, _a_addr, a_shutdown, _a_handle) = create_fast_test_node("node-fail-a", 30).await;
    let (_node_b, b_addr, _b_shutdown, b_handle) = create_fast_test_node("node-fail-b", 31).await;

    // 1. Node A bootstraps with Node B
    node_a.bootstrap(&[b_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    {
        let peers_a = node_a.peers.read().await;
        assert!(peers_a.contains_key("node-fail-b"), "node A should know node B");
    }

    // 2. Ungraceful crash of Node B (abort task without sending shutdown or leave datagram)
    b_handle.abort();

    // 3. Wait for Node A's Foca probe cycle and suspect_to_down_after timeout to expire
    // probe_period (60ms) + suspect_to_down_after (150ms) = ~210ms
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 4. Verify Node A declared Node B dead and removed it from routing
    {
        let peers_a = node_a.peers.read().await;
        assert!(
            !peers_a.contains_key("node-fail-b"),
            "node A must detect Node B failure after missed heartbeat TTL"
        );
        let wg_a = node_a.wireguard.read().await;
        assert_eq!(wg_a.peer_count(), 0);
    }

    // 5. Restart Node B (recovers)
    let (node_b_restarted, _b_restarted_addr, b_restarted_shutdown, _b_restarted_handle) =
        create_fast_test_node("node-fail-b", 31).await;

    // Node B announces to Node A
    node_b_restarted.bootstrap(&[_a_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 6. Verify Node A detected Node B recovery
    {
        let peers_a = node_a.peers.read().await;
        assert!(
            peers_a.contains_key("node-fail-b"),
            "node A must detect Node B recovery upon rejoin"
        );
        let wg_a = node_a.wireguard.read().await;
        assert_eq!(wg_a.peer_count(), 1);
    }

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_restarted_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_route_table_broadcast_instant_propagation() {
    let (node_a, _a_addr, a_shutdown) = create_test_node("node-route-a", 40).await;
    let (node_b, b_addr, b_shutdown) = create_test_node("node-route-b", 41).await;

    // 1. Node A bootstraps with Node B
    node_a.bootstrap(&[b_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Register service on Node B and broadcast it
    let service_route = Route::new(
        Some("127.0.0.1:3000".parse().unwrap()),
        Node::new("node-route-b", "10.8.0.41:443".parse().unwrap()),
    );
    node_b
        .broadcast_route("api.service.test".to_string(), service_route)
        .await
        .unwrap();

    // Settle network packet delivery
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Verify Node A can immediately resolve the route
    let resolved = node_a.registry.lookup("api.service.test");
    assert!(resolved.is_some(), "Node A must have received the route via broadcast");
    let route = resolved.unwrap();
    assert_eq!(route.node.node_id, "node-route-b");
    assert_eq!(route.upstream, Some("127.0.0.1:3000".parse().unwrap()));

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_route_table_push_pull_digest_convergence() {
    let (node_a, _a_addr, a_shutdown) = create_test_node("node-gossip-a", 50).await;
    let (node_b, b_addr, b_shutdown) = create_test_node("node-gossip-b", 51).await;

    // 1. Node A bootstraps with Node B
    node_a.bootstrap(&[b_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Node A has route "alpha.internal" and Node B has route "beta.internal" inserted locally
    node_a.registry.insert(
        "alpha.internal".to_string(),
        Route::new(
            Some("127.0.0.1:8001".parse().unwrap()),
            Node::new("node-gossip-a", "10.8.0.50:443".parse().unwrap()),
        ).with_clock("node-gossip-a", 1),
    );

    node_b.registry.insert(
        "beta.internal".to_string(),
        Route::new(
            Some("127.0.0.1:8002".parse().unwrap()),
            Node::new("node-gossip-b", "10.8.0.51:443".parse().unwrap()),
        ).with_clock("node-gossip-b", 1),
    );

    // Initial state: A does not have beta, B does not have alpha
    assert!(node_a.registry.lookup("beta.internal").is_none());
    assert!(node_b.registry.lookup("alpha.internal").is_none());

    // 3. Trigger push-pull digest gossip from Node A to Node B
    let sent = node_a.gossip_routes(1).await.unwrap();
    assert_eq!(sent, 1);

    // Settle bidirectional digest exchange (Digest -> Pull -> Sync)
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 4. Verify both nodes have converged
    assert!(node_a.registry.lookup("beta.internal").is_some(), "Node A must have pulled beta from Node B");
    assert!(node_b.registry.lookup("alpha.internal").is_some(), "Node B must have received alpha from Node A");

    assert_eq!(
        node_a.registry.lookup("beta.internal").unwrap().upstream,
        Some("127.0.0.1:8002".parse().unwrap())
    );
    assert_eq!(
        node_b.registry.lookup("alpha.internal").unwrap().upstream,
        Some("127.0.0.1:8001".parse().unwrap())
    );

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_route_table_tombstone_propagation() {
    let (node_a, _a_addr, a_shutdown) = create_test_node("node-tomb-a", 60).await;
    let (node_b, b_addr, b_shutdown) = create_test_node("node-tomb-b", 61).await;

    // 1. Both nodes start with "temp.service.test" (v1)
    let initial_route = Route::new(
        Some("127.0.0.1:9000".parse().unwrap()),
        Node::new("node-tomb-b", "10.8.0.61:443".parse().unwrap()),
    ).with_clock("node-tomb-b", 1);

    node_a.registry.insert("temp.service.test".to_string(), initial_route.clone());
    node_b.registry.insert("temp.service.test".to_string(), initial_route);

    // Node A bootstraps with Node B
    node_a.bootstrap(&[b_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. Node B decommissions the service by broadcasting a tombstone at v2
    let tombstone = Route::new(None, Node::new("node-tomb-b", "10.8.0.61:443".parse().unwrap()))
        .with_clock("node-tomb-b", 2)
        .tombstone();

    node_b.broadcast_route("temp.service.test".to_string(), tombstone).await.unwrap();

    // Settle broadcast delivery
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Verify Node A's lookup returns None (pruned via tombstone)
    assert!(node_a.registry.lookup("temp.service.test").is_none());
    // But raw lookup still holds tombstone with v2
    let raw = node_a.registry.lookup_raw("temp.service.test");
    assert!(raw.is_some());
    assert!(raw.unwrap().is_tombstone);

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
}

async fn create_election_test_node(
    node_id: &str,
    priority: u32,
    mesh_ip_last_octet: u8,
) -> (
    Arc<ClusterController>,
    SocketAddr,
    broadcast::Sender<ShutdownReason>,
    tokio::task::JoinHandle<()>,
) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local_addr = socket.local_addr().unwrap();

    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, mesh_ip_last_octet));
    let peer_node = PeerNode::new(
        node_id,
        format!("pubkey_{node_id}"),
        mesh_ip,
        local_addr,
    )
    .with_priority(priority);

    let wg = Arc::new(RwLock::new(WireGuardDevice::new(
        "wg0",
        format!("privkey_{node_id}"),
        format!("pubkey_{node_id}"),
        local_addr.port(),
        mesh_ip,
    )));

    let mut config = foca::Config::simple();
    config.probe_period = Duration::from_millis(60);
    config.probe_rtt = Duration::from_millis(20);
    config.suspect_to_down_after = Duration::from_millis(150);

    let registry = Arc::new(DomainRegistry::new());
    let controller = Arc::new(
        ClusterController::new_with_foca_config(peer_node, socket, wg, registry, config)
            .with_election_timeout(Duration::from_millis(80))
            .with_ack_timeout(Duration::from_millis(80)),
    );

    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let ctrl_clone = controller.clone();
    let handle = tokio::spawn(async move {
        let _ = ctrl_clone.run_with_shutdown(shutdown_rx, None).await;
    });

    (controller, local_addr, shutdown_tx, handle)
}

#[tokio::test]
async fn test_bully_election_highest_ranked_node_wins() {
    // 3 nodes: Node A ("node-1"), Node B ("node-2"), Node C ("node-3")
    // Rank: Node C > Node B > Node A
    let (node_a, _a_addr, a_shutdown, _a_h) = create_election_test_node("node-1", 0, 71).await;
    let (node_b, b_addr, b_shutdown, _b_h) = create_election_test_node("node-2", 0, 72).await;
    let (node_c, c_addr, c_shutdown, _c_h) = create_election_test_node("node-3", 0, 73).await;

    // Bootstrap mesh: A joins B, B joins C, A joins C
    node_a.bootstrap(&[b_addr]).await.unwrap();
    node_b.bootstrap(&[c_addr]).await.unwrap();
    node_a.bootstrap(&[c_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Node A (lowest rank) initiates election
    let elected = node_a.start_election().await.unwrap();
    assert!(!elected, "Node A should step down for higher peers");

    // Allow bully election to propagate (A -> B/C -> C declares Coordinator -> A/B ack)
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify Node C is the elected leader across all nodes
    assert!(node_c.is_leader().await, "Node C (highest ID) must be Leader");
    assert_eq!(
        node_a.current_leader().await.map(|l| l.node_id),
        Some("node-3".to_string())
    );
    assert_eq!(
        node_b.current_leader().await.map(|l| l.node_id),
        Some("node-3".to_string())
    );
    assert_eq!(
        node_c.current_leader().await.map(|l| l.node_id),
        Some("node-3".to_string())
    );

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
    let _ = c_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_bully_election_kill_leader_and_failover_within_ttl() {
    let (node_a, _a_addr, a_shutdown, _a_h) = create_election_test_node("failover-node-1", 0, 81).await;
    let (node_b, b_addr, b_shutdown, _b_h) = create_election_test_node("failover-node-2", 0, 82).await;
    let (node_c, c_addr, _c_shutdown, c_h) = create_election_test_node("failover-node-3", 0, 83).await;

    node_a.bootstrap(&[b_addr, c_addr]).await.unwrap();
    node_b.bootstrap(&[c_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Node C becomes initial leader
    node_c.start_election().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(node_c.is_leader().await);
    assert_eq!(
        node_a.current_leader().await.map(|l| l.node_id),
        Some("failover-node-3".to_string())
    );

    // Kill Node C (leader crash via ungraceful task abort)
    c_h.abort();

    // Wait for SWIM failure detection (probe_period 60ms + suspect 150ms = ~210ms)
    // plus election round (~160ms) -> total ~500ms
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify Node B (highest surviving) was elected new leader
    assert!(
        node_b.is_leader().await,
        "Node B must be promoted to leader upon Node C failure"
    );
    assert_eq!(
        node_a.current_leader().await.map(|l| l.node_id),
        Some("failover-node-2".to_string()),
        "Node A must acknowledge Node B as the new leader"
    );

    let _ = a_shutdown.send(ShutdownReason::Manual);
    let _ = b_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_bully_election_kill_two_nodes_quorum_behavior() {
    let (node_a, a_addr, a_shutdown, _a_h) = create_election_test_node("quorum-node-1", 0, 91).await;
    let (node_b, b_addr, _b_shutdown, b_h) = create_election_test_node("quorum-node-2", 0, 92).await;
    let (node_c, c_addr, _c_shutdown, c_h) = create_election_test_node("quorum-node-3", 0, 93).await;

    node_a.bootstrap(&[b_addr, c_addr]).await.unwrap();
    node_b.bootstrap(&[a_addr, c_addr]).await.unwrap();
    node_c.bootstrap(&[a_addr, b_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Node C elected leader
    node_c.start_election().await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(node_c.is_leader().await);

    // Fleet of 3 nodes: quorum requires (3 / 2) + 1 = 2
    // Kill two nodes: Node C and Node B
    c_h.abort();
    b_h.abort();

    // Settle failure detection
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node A remaining attempts election with original fleet size = 3
    node_a.set_total_known_nodes(3).await;
    let elected = node_a.start_election().await.unwrap();
    // Since only 1 node is alive, 1 < 2 -> Quorum fails!
    assert!(
        !elected,
        "Node A must not become leader when quorum (2) is not reachable"
    );
    assert!(
        !node_a.is_leader().await,
        "Node A must not assume leader role without quorum"
    );

    // But if quorum allows (e.g. operator sets quorum size to 1)
    node_a.set_quorum_size(Some(1)).await;
    let elected_solo = node_a.start_election().await.unwrap();
    assert!(elected_solo, "Node A can elect if quorum allows");
    assert!(node_a.is_leader().await);

    let _ = a_shutdown.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_bully_election_configured_priority_overrides_node_id() {
    // Node "alpha" has priority 100
    // Node "zeta" has priority 10
    // Lexicographically "zeta" > "alpha", but priority 100 > 10
    let (node_alpha, _alpha_addr, alpha_shutdown, _a_h) =
        create_election_test_node("alpha-server", 100, 101).await;
    let (node_zeta, zeta_addr, zeta_shutdown, _z_h) =
        create_election_test_node("zeta-server", 10, 102).await;

    node_alpha.bootstrap(&[zeta_addr]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Node zeta starts election
    let _ = node_zeta.start_election().await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Verify alpha won because its priority is 100 vs zeta's 10
    assert!(
        node_alpha.is_leader().await,
        "Node alpha (priority 100) must win over zeta (priority 10)"
    );
    assert_eq!(
        node_zeta.current_leader().await.map(|l| l.node_id),
        Some("alpha-server".to_string())
    );

    let _ = alpha_shutdown.send(ShutdownReason::Manual);
    let _ = zeta_shutdown.send(ShutdownReason::Manual);
}


