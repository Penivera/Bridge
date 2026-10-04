use std::sync::Arc;
use std::time::Duration;

use proxy::core::config::ProxyConfig;
use proxy::core::enums::{ProxyMode, Scheme};
use proxy::shutdown::{spawn_state_finalizer, ShutdownAck, ShutdownCoordinator, ShutdownReason};
use proxy::Proxy;
use registry::{DomainRegistry, Node, Route};
use tokio::net::TcpListener;

#[tokio::test]
async fn test_shutdown_coordinator_broadcast_and_ack() {
    let mut coordinator = ShutdownCoordinator::new(None);

    let (mut rx1, ack_tx1) = coordinator.register_subsystem("subsystem_alpha");
    let (mut rx2, ack_tx2) = coordinator.register_subsystem("subsystem_beta");

    // Subsystem 1: simulates state finalization
    tokio::spawn(async move {
        if let Ok(reason) = rx1.recv().await {
            assert_eq!(reason, ShutdownReason::Sigint);
            let _ = ack_tx1
                .send(ShutdownAck {
                    subsystem: "subsystem_alpha".to_string(),
                    details: Some("alpha_finalized".to_string()),
                })
                .await;
        }
    });

    // Subsystem 2: simulates file update and ack
    tokio::spawn(async move {
        if let Ok(reason) = rx2.recv().await {
            assert_eq!(reason, ShutdownReason::Sigint);
            let _ = ack_tx2
                .send(ShutdownAck {
                    subsystem: "subsystem_beta".to_string(),
                    details: Some("beta_state_saved".to_string()),
                })
                .await;
        }
    });

    let acks = coordinator
        .broadcast_and_wait(ShutdownReason::Sigint, Duration::from_secs(2))
        .await;

    assert_eq!(acks.len(), 2);
    let names: Vec<String> = acks.into_iter().map(|a| a.subsystem).collect();
    assert!(names.contains(&"subsystem_alpha".to_string()));
    assert!(names.contains(&"subsystem_beta".to_string()));
}

#[tokio::test]
async fn test_state_finalizer_persists_file_and_signals_ack() {
    let temp_dir = std::env::temp_dir();
    let state_file = temp_dir.join(format!("bridge-state-test-{}.json", std::process::id()));

    let mut coordinator = ShutdownCoordinator::new(Some(state_file.clone()));
    let (state_rx, ack_tx) = coordinator.register_subsystem("state_finalizer");

    let registry = Arc::new(DomainRegistry::new());
    registry.insert(
        "app.discovered.test".to_string(),
        Route::new(
            Some("127.0.0.1:8080".parse().unwrap()),
            Node::new("vm-dyn", "10.8.0.50:443".parse().unwrap()),
        ),
    );

    let udp_service = proxy::core::config::UdpServiceConfig::new(
        "127.0.0.1:5353".parse().unwrap(),
        "127.0.0.1:53".parse().unwrap(),
    );

    spawn_state_finalizer(
        state_file.clone(),
        "Handoff".to_string(),
        registry,
        vec![udp_service],
        state_rx,
        ack_tx,
    );

    let acks = coordinator
        .broadcast_and_wait(ShutdownReason::Sigterm, Duration::from_secs(2))
        .await;

    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0].subsystem, "state_finalizer");

    // Verify state file was written to disk with full routing and service data
    let content = tokio::fs::read_to_string(&state_file).await.unwrap();
    let state: proxy::RuntimeState = serde_json::from_str(&content).expect("failed to deserialize runtime state");

    assert_eq!(state.proxy_mode, "Handoff");
    assert_eq!(state.routes_count, 1);
    assert!(state.routes.contains_key("app.discovered.test"));
    assert_eq!(state.routes["app.discovered.test"].node.node_id, "vm-dyn");
    assert_eq!(state.udp_services.len(), 1);
    assert_eq!(state.status, "gracefully_stopped");

    let _ = tokio::fs::remove_file(state_file).await;
}

#[tokio::test]
async fn test_shutdown_ack_timeout_protection() {
    let mut coordinator = ShutdownCoordinator::new(None);
    let (_rx, _ack_tx) = coordinator.register_subsystem("hanging_subsystem");
    // Do not respond with ack

    let start = std::time::Instant::now();
    let acks = coordinator
        .broadcast_and_wait(ShutdownReason::Manual, Duration::from_millis(50))
        .await;

    assert!(acks.is_empty());
    assert!(start.elapsed() >= Duration::from_millis(40));
}

#[tokio::test]
async fn test_proxy_run_with_coordinator_graceful_teardown() {
    let temp_dir = std::env::temp_dir();
    let state_file = temp_dir.join(format!("bridge-proxy-state-{}.json", std::process::id()));

    let mut coordinator = ShutdownCoordinator::new(Some(state_file.clone()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let registry = Arc::new(DomainRegistry::new());
    registry.insert("test.local".to_string(), Route::new(None, Node::new("vm1", addr)));

    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        https_addr: addr,
        listeners: vec![Scheme::Https],
        ..Default::default()
    };
    let proxy = Proxy::new(Arc::new(proxy_config), registry);

    let sender = coordinator.sender();

    let run_handle = tokio::spawn(async move {
        proxy.run_with_coordinator(&mut coordinator).await
    });

    // Allow listeners to bind and start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send broadcast shutdown
    let _ = sender.send(ShutdownReason::Manual);

    // Verify proxy shuts down cleanly
    let result = tokio::time::timeout(Duration::from_secs(3), run_handle)
        .await
        .expect("proxy shutdown timed out")
        .expect("proxy task panicked");

    assert!(result.is_ok());

    // Verify state file was written and contains full state
    let content = tokio::fs::read_to_string(&state_file).await.unwrap();
    let state: proxy::RuntimeState = serde_json::from_str(&content).expect("failed to deserialize runtime state");
    assert_eq!(state.proxy_mode, "Handoff");
    assert_eq!(state.routes_count, 1);
    assert_eq!(state.status, "gracefully_stopped");

    let _ = tokio::fs::remove_file(state_file).await;
}

#[tokio::test]
async fn test_runtime_discovered_route_captured_on_shutdown() {
    let temp_dir = std::env::temp_dir();
    let state_file = temp_dir.join(format!("bridge-discovered-routes-{}.yaml", std::process::id()));

    let mut coordinator = ShutdownCoordinator::new(Some(state_file.clone()));

    let registry = Arc::new(DomainRegistry::new());
    // Initial static route
    registry.insert(
        "static.test".to_string(),
        Route::new(None, Node::new("node-static", "10.0.0.1:443".parse().unwrap())),
    );

    let (state_rx, ack_tx) = coordinator.register_subsystem("state_finalizer");
    spawn_state_finalizer(
        state_file.clone(),
        "Handoff".to_string(),
        registry.clone(),
        vec![],
        state_rx,
        ack_tx,
    );

    // Simulate runtime discovery: a new route is dynamically registered at runtime
    registry.insert(
        "runtime-discovered.coolify.test".to_string(),
        Route::new(
            Some("192.168.1.100:8443".parse().unwrap()),
            Node::new("node-discovered", "10.8.0.99:443".parse().unwrap())
                .with_direct_address("203.0.113.99:443".parse().unwrap())
                .with_routing(registry::RoutingPreference::Direct),
        ),
    );

    // Trigger shutdown
    let acks = coordinator
        .broadcast_and_wait(ShutdownReason::Sigint, Duration::from_secs(2))
        .await;
    assert_eq!(acks.len(), 1);

    // Read YAML state file
    let yaml_content = tokio::fs::read_to_string(&state_file).await.unwrap();
    let state: proxy::RuntimeState =
        serde_yaml::from_str(&yaml_content).expect("failed to deserialize YAML runtime state");

    assert_eq!(state.routes_count, 2);
    assert!(state.routes.contains_key("static.test"));
    assert!(state.routes.contains_key("runtime-discovered.coolify.test"));

    let dyn_route = &state.routes["runtime-discovered.coolify.test"];
    assert_eq!(dyn_route.node.node_id, "node-discovered");
    assert_eq!(
        dyn_route.upstream,
        Some("192.168.1.100:8443".parse().unwrap())
    );
    assert_eq!(
        dyn_route.node.direct_address,
        Some("203.0.113.99:443".parse().unwrap())
    );

    let _ = tokio::fs::remove_file(state_file).await;
}
