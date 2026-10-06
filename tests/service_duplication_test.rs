use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bridge::cluster::{ClusterController, PeerNode, WorkloadCommand};
use bridge::core::config::{Config, FailbackMode, Service, ServiceReplicationConfig};
use bridge::failover::{MockContainerDriver, WorkloadDuplicator};
use bridge::mesh::WireGuardDevice;
use proxy::ShutdownReason;
use registry::{DomainRegistry, Node, Route};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, RwLock};
use url::Url;

/// Helper to create a test `ClusterController` with an ephemeral UDP socket.
async fn create_test_controller(
    node_id: &str,
    mesh_ip_last_octet: u8,
) -> (Arc<ClusterController>, PeerNode) {
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
    let controller = Arc::new(ClusterController::new_with_socket(
        peer_node.clone(),
        socket,
        wg,
        registry,
    ));

    (controller, peer_node)
}

#[test]
fn test_config_deserialization_replicate() {
    let toml_data = r#"
        [[services]]
        url = "http://api.example.com"
        node_id = "worker-1"
        upstream = "127.0.0.1:8080"
        [services.replicate]
        enabled = true
        image = "myorg/api:v2.0"
        env = ["PORT=8080", "MODE=replica"]
        container_port = 8080
        placement = "leader"
        failback_mode = "preemptive"
        failback_cooldown_secs = 10

        [[services]]
        url = "http://db.example.com"
        node_id = "worker-2"
        upstream = "127.0.0.1:5432"
        [services.replicate]
        enabled = true
        failback_mode = "manual"
    "#;

    let config = Config::from_toml_str(toml_data).expect("parse toml config");
    assert_eq!(config.services.len(), 2);

    let s1 = &config.services[0];
    let repl1 = s1.replicate.as_ref().expect("service 1 replicate config");
    assert!(repl1.enabled);
    assert_eq!(repl1.image.as_deref(), Some("myorg/api:v2.0"));
    assert_eq!(repl1.env, vec!["PORT=8080", "MODE=replica"]);
    assert_eq!(repl1.container_port, Some(8080));
    assert_eq!(repl1.placement, "leader");
    assert_eq!(repl1.failback_mode, FailbackMode::Preemptive);
    assert_eq!(repl1.failback_cooldown_secs, 10);

    let s2 = &config.services[1];
    let repl2 = s2.replicate.as_ref().expect("service 2 replicate config");
    assert!(repl2.enabled);
    assert_eq!(repl2.failback_mode, FailbackMode::Manual);
    assert_eq!(repl2.placement, "ring"); // default
    assert_eq!(repl2.failback_cooldown_secs, 10); // default
}

#[tokio::test]
async fn test_non_leader_ignores_member_down() {
    let (ctrl, local_node) = create_test_controller("node-follower", 1).await;
    // Set leader to a different node
    let leader_peer = PeerNode::new(
        "node-leader",
        "pubkey_leader",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 10)),
        "127.0.0.1:19999".parse().unwrap(),
    );
    ctrl.election_state.write().await.current_leader = Some(leader_peer);

    let dead_node = PeerNode::new(
        "node-dead",
        "pubkey_dead",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://api.example.com").unwrap(),
        node_id: "node-dead".to_string(),
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("api:latest".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::NonPreemptive,
            failback_cooldown_secs: 30,
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    // Follower should ignore dead member event
    let spawned = duplicator
        .handle_member_down(&dead_node)
        .await
        .expect("handle_member_down");
    assert_eq!(spawned, 0);
    assert!(!mock_driver.is_running("api.example.com").await);
    assert_eq!(duplicator.active_duplications().await.len(), 0);
}

#[tokio::test]
async fn test_leader_duplicates_service_locally_on_failure() {
    let (ctrl, local_node) = create_test_controller("node-leader", 1).await;
    // Mark local node as leader
    ctrl.election_state.write().await.current_leader = Some(local_node.clone());

    let dead_node = PeerNode::new(
        "node-worker-1",
        "pubkey_worker",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    // Initial route points to worker
    let worker_route = Route::new(
        Some("127.0.0.1:8080".parse().unwrap()),
        Node::new(&dead_node.node_id, dead_node.endpoint),
    );
    ctrl.registry.insert("api.example.com".to_string(), worker_route);

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://api.example.com").unwrap(),
        node_id: "node-worker-1".to_string(),
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("api:v1.0".to_string()),
            env: vec!["ENV=test".to_string()],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::Manual,
            failback_cooldown_secs: 30,
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    let spawned = duplicator
        .handle_member_down(&dead_node)
        .await
        .expect("handle_member_down");
    assert_eq!(spawned, 1);

    // Verify container spawned locally
    assert!(mock_driver.is_running("api.example.com").await);
    let spawned_addr = mock_driver.spawned_addr("api.example.com").await.unwrap();

    // Verify route in registry now points to leader's duplicate
    let updated_route = ctrl.registry.lookup("api.example.com").expect("route exists");
    assert_eq!(updated_route.target_addr(), spawned_addr);

    // Verify active duplication tracking
    let dups = duplicator.active_duplications().await;
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].domain, "api.example.com");
    assert_eq!(dups[0].origin_node_id, "node-worker-1");
    assert_eq!(dups[0].assigned_node_id, "node-leader");
    assert_eq!(dups[0].failback_mode, FailbackMode::Manual);
    assert_eq!(dups[0].spawned_addr, spawned_addr);
}

#[tokio::test]
async fn test_non_preemptive_failback_mode() {
    let (ctrl, local_node) = create_test_controller("node-leader", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_node.clone());

    let worker_node = PeerNode::new(
        "node-worker-1",
        "pubkey_worker",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://api.example.com").unwrap(),
        node_id: "node-worker-1".to_string(),
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("api:v1.0".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::NonPreemptive,
            failback_cooldown_secs: 5,
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    // Node fails -> duplicate spawned
    let spawned = duplicator.handle_member_down(&worker_node).await.unwrap();
    assert_eq!(spawned, 1);
    assert!(mock_driver.is_running("api.example.com").await);

    // Node recovers -> NonPreemptive mode should NOT stop the duplicate
    let recovered_count = duplicator.handle_member_up(&worker_node).await.unwrap();
    assert_eq!(recovered_count, 1);

    // Container should remain running
    assert!(mock_driver.is_running("api.example.com").await);
    assert_eq!(duplicator.active_duplications().await.len(), 1);
}

#[tokio::test]
async fn test_preemptive_failback_mode_with_cooldown() {
    let (ctrl, local_node) = create_test_controller("node-leader", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_node.clone());

    let worker_node = PeerNode::new(
        "node-worker-1",
        "pubkey_worker",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    // Store original route
    let original_target: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let worker_route = Route::new(
        Some(original_target),
        Node::new(&worker_node.node_id, worker_node.endpoint),
    );
    ctrl.registry.insert("api.example.com".to_string(), worker_route);

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://api.example.com").unwrap(),
        node_id: "node-worker-1".to_string(),
        upstream: Some(original_target),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("api:v1.0".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::Preemptive,
            failback_cooldown_secs: 1, // 1-second cooldown for fast test
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    // 1. Node fails -> duplicate spawned
    let spawned = duplicator.handle_member_down(&worker_node).await.unwrap();
    assert_eq!(spawned, 1);
    assert!(mock_driver.is_running("api.example.com").await);

    // 2. Node recovers -> re-add to cluster peers so it is considered alive
    ctrl.peers.write().await.insert(worker_node.node_id.clone(), worker_node.clone());

    let recovered_count = duplicator.handle_member_up(&worker_node).await.unwrap();
    assert_eq!(recovered_count, 1);

    // 3. Immediately after recover event, container is still running during cooldown
    assert!(mock_driver.is_running("api.example.com").await);

    // 4. Wait for 1-second cooldown to elapse + margin
    tokio::time::sleep(Duration::from_millis(1300)).await;

    // 5. Container should now be stopped
    assert!(!mock_driver.is_running("api.example.com").await);
    assert_eq!(duplicator.active_duplications().await.len(), 0);

    // 6. Registry route should be restored to worker
    let restored_route = ctrl.registry.lookup("api.example.com").expect("route restored");
    assert_eq!(restored_route.target_addr(), original_target);
}

#[tokio::test]
async fn test_manual_failback_mode() {
    let (ctrl, local_node) = create_test_controller("node-leader", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_node.clone());

    let worker_node = PeerNode::new(
        "node-worker-1",
        "pubkey_worker",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    let original_target: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let worker_route = Route::new(
        Some(original_target),
        Node::new(&worker_node.node_id, worker_node.endpoint),
    );
    ctrl.registry.insert("api.example.com".to_string(), worker_route);

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://api.example.com").unwrap(),
        node_id: "node-worker-1".to_string(),
        upstream: Some(original_target),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("api:v1.0".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::Manual,
            failback_cooldown_secs: 30,
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    // Node fails -> duplicate spawned
    let spawned = duplicator.handle_member_down(&worker_node).await.unwrap();
    assert_eq!(spawned, 1);
    assert!(mock_driver.is_running("api.example.com").await);

    // Node recovers -> re-add to peers
    ctrl.peers.write().await.insert(worker_node.node_id.clone(), worker_node.clone());
    let _ = duplicator.handle_member_up(&worker_node).await.unwrap();

    // Container still running under Manual mode
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(mock_driver.is_running("api.example.com").await);
    assert_eq!(duplicator.active_duplications().await.len(), 1);

    // Explicit operator manual failback
    let failback_res = duplicator.execute_failback("api.example.com").await.unwrap();
    assert!(failback_res);

    // Container stopped and active duplications cleared
    assert!(!mock_driver.is_running("api.example.com").await);
    assert_eq!(duplicator.active_duplications().await.len(), 0);

    // Route restored
    let restored_route = ctrl.registry.lookup("api.example.com").expect("route restored");
    assert_eq!(restored_route.target_addr(), original_target);
}

#[tokio::test]
async fn test_ring_placement_strategy() {
    let (ctrl, local_node) = create_test_controller("node-leader", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_node.clone());

    // Add another healthy survivor peer to the cluster
    let peer_node = PeerNode::new(
        "node-survivor",
        "pubkey_survivor",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 3)),
        "127.0.0.1:18003".parse().unwrap(),
    );
    ctrl.peers.write().await.insert(peer_node.node_id.clone(), peer_node.clone());

    let dead_node = PeerNode::new(
        "node-dead",
        "pubkey_dead",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:18002".parse().unwrap(),
    );

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://service.example.com").unwrap(),
        node_id: "node-dead".to_string(),
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("service:latest".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "ring".to_string(),
            failback_mode: FailbackMode::NonPreemptive,
            failback_cooldown_secs: 30,
        }),
    }];

    let duplicator = WorkloadDuplicator::new(
        local_node.node_id.clone(),
        services,
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    );

    let spawned = duplicator
        .handle_member_down(&dead_node)
        .await
        .expect("handle_member_down");
    assert_eq!(spawned, 1);

    let dups = duplicator.active_duplications().await;
    assert_eq!(dups.len(), 1);
    // Target node should be either leader ("node-leader") or the survivor ("node-survivor")
    assert!(
        dups[0].assigned_node_id == "node-leader" || dups[0].assigned_node_id == "node-survivor",
        "assigned node must be one of healthy nodes"
    );
}

#[tokio::test]
async fn test_remote_workload_commands_dispatch_and_execution() {
    let (ctrl, local_node) = create_test_controller("worker-node", 2).await;
    let mock_driver = Arc::new(MockContainerDriver::new());

    let duplicator = Arc::new(WorkloadDuplicator::new(
        local_node.node_id.clone(),
        vec![],
        ctrl.registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    ));

    let (_member_tx, member_rx) = broadcast::channel(16);
    let (command_tx, command_rx) = broadcast::channel(16);
    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);

    let dup_clone = duplicator.clone();
    let runner_handle = tokio::spawn(async move {
        let _ = dup_clone
            .run_with_shutdown(member_rx, command_rx, shutdown_rx, None)
            .await;
    });

    // 1. Send WorkloadCommand::Spawn to worker
    command_tx
        .send(WorkloadCommand::Spawn {
            domain: "app.example.com".to_string(),
            image: "app:v1".to_string(),
            env: vec!["VAR=1".to_string()],
            container_port: 8080,
            origin_node: "dead-node".to_string(),
            from_node: "leader-node".to_string(),
        })
        .expect("send spawn command");

    // Allow event loop to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify container spawned on worker
    assert!(mock_driver.is_running("app.example.com").await);
    let spawned_addr = mock_driver.spawned_addr("app.example.com").await.unwrap();

    // Verify route in worker's registry
    let route = ctrl.registry.lookup("app.example.com").expect("route created");
    assert_eq!(route.target_addr(), spawned_addr);

    // 2. Send WorkloadCommand::Stop to worker
    command_tx
        .send(WorkloadCommand::Stop {
            domain: "app.example.com".to_string(),
            from_node: "leader-node".to_string(),
        })
        .expect("send stop command");

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify container stopped on worker
    assert!(!mock_driver.is_running("app.example.com").await);

    // Clean shutdown
    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = runner_handle.await;
}
