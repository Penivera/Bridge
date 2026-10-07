use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bridge::cli::{Args, Commands, ElectionCommands, MeshCommands, NodeCommands, RoutesCommands};
use bridge::cluster::{ClusterController, ElectionRole, PeerNode};
use bridge::ipc::{IpcClient, IpcData, IpcServer};
use bridge::mesh::WireGuardDevice;
use clap::Parser;
use proxy::core::enums::ProxyMode;
use proxy::ShutdownReason;
use registry::{DomainRegistry, Node, Route};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, RwLock};

async fn create_test_cluster(
    local_id: &str,
    mesh_octet: u8,
) -> (Arc<ClusterController>, PeerNode) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local_addr = socket.local_addr().unwrap();
    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, mesh_octet));
    let peer_node = PeerNode::new(
        local_id,
        format!("pubkey_{local_id}"),
        mesh_ip,
        local_addr,
    );
    let wg = Arc::new(RwLock::new(WireGuardDevice::new(
        "wg0",
        format!("privkey_{local_id}"),
        format!("pubkey_{local_id}"),
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

#[tokio::test]
async fn test_admin_force_leader_and_step_down() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-admin-leader-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());
    let ipc_server = IpcServer::new(&socket_path, registry, ProxyMode::Handoff);

    let (ctrl, _local_peer) = create_test_cluster("node-01", 1).await;
    let remote_peer = PeerNode::new(
        "node-02",
        "pubkey_node02",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:51822".parse().unwrap(),
    );
    ctrl.peers
        .write()
        .await
        .insert(remote_peer.node_id.clone(), remote_peer.clone());

    *ipc_server.cluster_handle().write().await = Some(ctrl.clone());

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC socket");

    // 1. Force leader to local node
    let resp = client.set_leader("node-01".to_string()).await.expect("set_leader node-01");
    match resp {
        IpcData::AdminResult { action, target, message } => {
            assert_eq!(action, "set_leader");
            assert_eq!(target, "node-01");
            assert!(message.contains("node-01"));
        }
        other => panic!("expected AdminResult, got {:?}", other),
    }

    assert_eq!(ctrl.election_state.read().await.role, ElectionRole::Leader);
    assert_eq!(
        ctrl.election_state.read().await.current_leader.as_ref().map(|l| l.node_id.as_str()),
        Some("node-01")
    );

    // 2. Step down from leadership
    let step_resp = client.step_down().await.expect("step_down");
    match step_resp {
        IpcData::AdminResult { action, target, message } => {
            assert_eq!(action, "step_down");
            assert_eq!(target, "node-01");
            assert!(message.contains("stepped down"));
        }
        other => panic!("expected AdminResult, got {:?}", other),
    }

    assert_eq!(ctrl.election_state.read().await.role, ElectionRole::Follower);

    // 3. Force leader to remote node
    let resp_remote = client.set_leader("node-02".to_string()).await.expect("set_leader node-02");
    match resp_remote {
        IpcData::AdminResult { action, target, message } => {
            assert_eq!(action, "set_leader");
            assert_eq!(target, "node-02");
            assert!(message.contains("node-02"));
        }
        other => panic!("expected AdminResult, got {:?}", other),
    }

    assert_eq!(ctrl.election_state.read().await.role, ElectionRole::Follower);
    assert_eq!(
        ctrl.election_state.read().await.current_leader.as_ref().map(|l| l.node_id.as_str()),
        Some("node-02")
    );

    // 4. Force leader to nonexistent node returns error
    let err = client.set_leader("unknown-vm".to_string()).await;
    assert!(err.is_err());

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_admin_drop_node_and_prune_routes() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-admin-drop-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());

    let (ctrl, local_peer) = create_test_cluster("node-01", 1).await;
    let remote_peer = PeerNode::new(
        "node-02",
        "pubkey_node02",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:51822".parse().unwrap(),
    );
    ctrl.peers
        .write()
        .await
        .insert(remote_peer.node_id.clone(), remote_peer.clone());

    // Add route on local node
    let r_local = Route::new(
        Some("127.0.0.1:3000".parse().unwrap()),
        Node::new(local_peer.node_id.clone(), "127.0.0.1:3000".parse().unwrap()),
    );
    ctrl.registry.insert("local.example.com".to_string(), r_local.clone());
    registry.insert("local.example.com".to_string(), r_local);

    // Add route on remote peer
    let r_remote = Route::new(
        Some("10.8.0.2:3000".parse().unwrap()),
        Node::new(remote_peer.node_id.clone(), "10.8.0.2:3000".parse().unwrap()),
    );
    ctrl.registry.insert("remote.example.com".to_string(), r_remote.clone());
    registry.insert("remote.example.com".to_string(), r_remote);

    // Add orphaned route targeting nonexistent node-03
    let r_stale = Route::new(
        Some("10.8.0.3:3000".parse().unwrap()),
        Node::new("node-03".to_string(), "10.8.0.3:3000".parse().unwrap()),
    );
    ctrl.registry.insert("stale.example.com".to_string(), r_stale.clone());
    registry.insert("stale.example.com".to_string(), r_stale);

    let ipc_server = IpcServer::new(&socket_path, registry.clone(), ProxyMode::Handoff);
    *ipc_server.cluster_handle().write().await = Some(ctrl.clone());

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC socket");

    // 1. Prune routes removes stale route (node-03)
    let prune_resp = client.prune_routes().await.expect("prune_routes");
    match prune_resp {
        IpcData::PruneResult { pruned_count, pruned_domains, .. } => {
            assert_eq!(pruned_count, 1);
            assert_eq!(pruned_domains, vec!["stale.example.com".to_string()]);
        }
        other => panic!("expected PruneResult, got {:?}", other),
    }
    assert!(registry.lookup("stale.example.com").is_none());
    assert!(registry.lookup("local.example.com").is_some());
    assert!(registry.lookup("remote.example.com").is_some());

    // 2. Drop node-02
    let drop_resp = client.drop_node("node-02".to_string()).await.expect("drop node-02");
    match drop_resp {
        IpcData::AdminResult { action, target, message } => {
            assert_eq!(action, "drop_node");
            assert_eq!(target, "node-02");
            assert!(message.contains("node-02"));
        }
        other => panic!("expected AdminResult, got {:?}", other),
    }

    assert!(!ctrl.peers.read().await.contains_key("node-02"));
    // Route for dropped node was also pruned by drop_node in ctrl.registry
    assert!(ctrl.registry.lookup("remote.example.com").is_none());

    // 3. Drop local node returns error
    let err = client.drop_node("node-01".to_string()).await;
    assert!(err.is_err());

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_admin_mesh_sync() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-admin-sync-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());
    let ipc_server = IpcServer::new(&socket_path, registry, ProxyMode::Handoff);

    let (ctrl, _local_peer) = create_test_cluster("node-01", 1).await;
    *ipc_server.cluster_handle().write().await = Some(ctrl.clone());

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC socket");

    let sync_resp = client.mesh_sync().await.expect("mesh_sync");
    match sync_resp {
        IpcData::MeshSyncResult { message, .. } => {
            assert!(message.contains("WireGuard"));
        }
        other => panic!("expected MeshSyncResult, got {:?}", other),
    }

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn test_cli_parsing_for_admin_commands() {
    // 1. election set-leader
    let args = Args::parse_from(["bridge", "election", "set-leader", "vm-02"]);
    match args.command {
        Some(Commands::Election { command: Some(ElectionCommands::SetLeader { node_id, .. }), .. }) => {
            assert_eq!(node_id, "vm-02");
        }
        other => panic!("expected ElectionCommands::SetLeader, got {:?}", other),
    }

    // 2. election step-down
    let args = Args::parse_from(["bridge", "election", "step-down"]);
    match args.command {
        Some(Commands::Election { command: Some(ElectionCommands::StepDown { .. }), .. }) => {}
        other => panic!("expected ElectionCommands::StepDown, got {:?}", other),
    }

    // 3. node drop
    let args = Args::parse_from(["bridge", "node", "drop", "vm-03"]);
    match args.command {
        Some(Commands::Node { command: NodeCommands::Drop { node_id, .. } }) => {
            assert_eq!(node_id, "vm-03");
        }
        other => panic!("expected NodeCommands::Drop, got {:?}", other),
    }

    // 4. routes prune
    let args = Args::parse_from(["bridge", "routes", "prune"]);
    match args.command {
        Some(Commands::Routes { command: Some(RoutesCommands::Prune { .. }), .. }) => {}
        other => panic!("expected RoutesCommands::Prune, got {:?}", other),
    }

    // 5. routes (default)
    let args = Args::parse_from(["bridge", "routes"]);
    match args.command {
        Some(Commands::Routes { command: None, .. }) => {}
        other => panic!("expected Commands::Routes with None command, got {:?}", other),
    }

    // 6. mesh sync
    let args = Args::parse_from(["bridge", "mesh", "sync"]);
    match args.command {
        Some(Commands::Mesh { command: Some(MeshCommands::Sync { .. }), .. }) => {}
        other => panic!("expected MeshCommands::Sync, got {:?}", other),
    }
}
