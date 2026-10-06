use std::sync::Arc;
use std::time::Duration;

use bridge::cluster::PeerNode;
use bridge::core::config::{
    resolve_env_str, Config, HandoffMode, TunnelHandoffConfig,
};
use bridge::handoff::TunnelManager;
use proxy::ShutdownReason;
use tokio::sync::{broadcast, mpsc, watch};

#[test]
fn test_tunnel_handoff_config_deserialization() {
    let toml_data = r#"
        [handoff]
        mode = "tunnel"

        [handoff.tunnel]
        token = "env:CF_TUNNEL_TOKEN"
        warm_standby = true
        binary_path = "/usr/bin/cloudflared"
        extra_args = ["--no-autoupdate", "--protocol", "quic"]
    "#;

    let config = Config::from_toml_str(toml_data).expect("parse toml config");
    assert_eq!(config.handoff.mode, HandoffMode::Tunnel);

    let tunnel = config.handoff.tunnel.expect("tunnel config present");
    assert_eq!(tunnel.token.as_deref(), Some("env:CF_TUNNEL_TOKEN"));
    assert!(tunnel.warm_standby);
    assert_eq!(tunnel.binary_path, "/usr/bin/cloudflared");
    assert_eq!(
        tunnel.extra_args,
        vec!["--no-autoupdate".to_string(), "--protocol".to_string(), "quic".to_string()]
    );
}

#[test]
fn test_tunnel_handoff_env_resolution() {
    unsafe {
        std::env::set_var("CF_TEST_SECRET_TOKEN", "mock_eyJhGciOiJKV1QiLCJhbGciOi...");
    }

    assert_eq!(
        resolve_env_str("env:CF_TEST_SECRET_TOKEN"),
        "mock_eyJhGciOiJKV1QiLCJhbGciOi..."
    );
    assert_eq!(
        resolve_env_str("literal_raw_token_value"),
        "literal_raw_token_value"
    );
}

#[tokio::test]
async fn test_tunnel_manager_start_and_stop_lifecycle() {
    let cfg = TunnelHandoffConfig {
        token: Some("mock_token".to_string()),
        tunnel_id: None,
        credentials_file: None,
        warm_standby: false,
        binary_path: "sleep".to_string(),
        extra_args: vec!["30".to_string()],
    };

    let manager = TunnelManager::new(cfg, "node-test-1");
    assert!(!manager.is_running().await);

    // 1. Start tunnel child process
    let pid = manager.start().await.expect("start mock tunnel process");
    assert!(pid > 0);
    assert!(manager.is_running().await);

    // 2. Stop tunnel process gracefully
    manager.stop().await.expect("stop mock tunnel process");
    assert!(!manager.is_running().await);
}

#[tokio::test]
async fn test_tunnel_manager_reactive_leadership_handoff() {
    let cfg = TunnelHandoffConfig {
        token: Some("mock_token".to_string()),
        tunnel_id: None,
        credentials_file: None,
        warm_standby: false, // Cold standby: only runs when leader
        binary_path: "sleep".to_string(),
        extra_args: vec!["30".to_string()],
    };

    let manager = Arc::new(TunnelManager::new(cfg, "leader-node"));
    let (leader_tx, leader_rx) = watch::channel::<Option<PeerNode>>(None);
    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let (ack_tx, mut ack_rx) = mpsc::channel(4);

    let mgr_clone = manager.clone();
    tokio::spawn(async move {
        let _ = mgr_clone
            .run_with_shutdown(leader_rx, shutdown_rx, Some(ack_tx))
            .await;
    });

    // 1. Initial state: no leader -> process should not be running
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!manager.is_running().await);

    // 2. Local node is elected leader -> cloudflared starts
    let local_peer = PeerNode::new(
        "leader-node",
        "pubkey_1",
        "10.8.0.1".parse().unwrap(),
        "127.0.0.1:51820".parse().unwrap(),
    );
    leader_tx.send(Some(local_peer)).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        manager.is_running().await,
        "tunnel process must start when local node is promoted to leader"
    );

    // 3. Local node is demoted (another node becomes leader) -> cloudflared stops (cold standby)
    let remote_peer = PeerNode::new(
        "other-node",
        "pubkey_2",
        "10.8.0.2".parse().unwrap(),
        "127.0.0.1:51821".parse().unwrap(),
    );
    leader_tx.send(Some(remote_peer)).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !manager.is_running().await,
        "tunnel process must stop when local node is demoted from leader"
    );

    // 4. Local node is promoted again -> cloudflared restarts
    let local_peer_re_elected = PeerNode::new(
        "leader-node",
        "pubkey_1",
        "10.8.0.1".parse().unwrap(),
        "127.0.0.1:51820".parse().unwrap(),
    );
    leader_tx.send(Some(local_peer_re_elected)).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(manager.is_running().await);

    // 5. Graceful shutdown broadcast
    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let ack = tokio::time::timeout(Duration::from_millis(500), ack_rx.recv())
        .await
        .expect("ack timeout")
        .expect("received ack");
    assert_eq!(ack.subsystem, "tunnel_handoff");
    assert!(!manager.is_running().await);
}

#[tokio::test]
async fn test_tunnel_manager_warm_standby_mode() {
    let cfg = TunnelHandoffConfig {
        token: Some("mock_token".to_string()),
        tunnel_id: None,
        credentials_file: None,
        warm_standby: true, // Warm standby: stays running regardless of leadership
        binary_path: "sleep".to_string(),
        extra_args: vec!["30".to_string()],
    };

    let manager = Arc::new(TunnelManager::new(cfg, "standby-node"));
    let (leader_tx, leader_rx) = watch::channel::<Option<PeerNode>>(None);
    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);

    let mgr_clone = manager.clone();
    tokio::spawn(async move {
        let _ = mgr_clone.run_with_shutdown(leader_rx, shutdown_rx, None).await;
    });

    // 1. Warm standby starts process immediately on daemon launch
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        manager.is_running().await,
        "warm standby must run cloudflared even without leadership"
    );

    // 2. When another node is elected leader, warm standby stays active
    let remote_peer = PeerNode::new(
        "remote-leader",
        "pubkey_remote",
        "10.8.0.9".parse().unwrap(),
        "127.0.0.1:51829".parse().unwrap(),
    );
    leader_tx.send(Some(remote_peer)).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        manager.is_running().await,
        "warm standby process must persist when follower"
    );

    // 3. Shutdown terminates it
    let _ = shutdown_tx.send(ShutdownReason::Sigterm);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!manager.is_running().await);
}
