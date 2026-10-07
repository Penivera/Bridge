use std::sync::Arc;
use std::time::Duration;

use bridge::ipc::{IpcClient, IpcData, IpcServer};
use proxy::core::enums::ProxyMode;
use registry::{DomainRegistry, Node, Route};
use tokio::sync::broadcast;

fn temp_socket_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("bridge-test-{}-{}.sock", name, std::process::id()));
    p
}

#[tokio::test]
async fn test_ipc_server_bind_and_ping() {
    let socket = temp_socket_path("ping");
    let registry = Arc::new(DomainRegistry::new());
    let server = IpcServer::new(&socket, registry, ProxyMode::Handoff);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket).await.expect("connect to IPC server");
    let pong = client.ping().await.expect("ping response");
    assert_eq!(pong, "pong");

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
    assert!(!socket.exists(), "socket file should be cleaned up on shutdown");
}

#[tokio::test]
async fn test_ipc_status() {
    let socket = temp_socket_path("status");
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "app.example.com".to_string(),
        Route::new(
            Some("127.0.0.1:3000".parse().unwrap()),
            Node::new("self", "127.0.0.1:3000".parse().unwrap()),
        ),
    );
    let registry = Arc::new(DomainRegistry::with_routes(routes));
    let server = IpcServer::new(&socket, registry, ProxyMode::Managed);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket).await.unwrap();
    let status_data = client.status().await.unwrap();

    match status_data {
        IpcData::Status {
            routes_count,
            proxy_mode,
            version,
            ..
        } => {
            assert_eq!(routes_count, 1);
            assert_eq!(proxy_mode, "Managed");
            assert!(!version.is_empty());
        }
        other => panic!("expected IpcData::Status, got {other:?}"),
    }

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
}

#[tokio::test]
async fn test_ipc_add_list_and_remove_route() {
    let socket = temp_socket_path("routes");
    let registry = Arc::new(DomainRegistry::new());
    let server = IpcServer::new(&socket, registry.clone(), ProxyMode::Handoff);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket).await.unwrap();

    // 1. Initially empty
    let routes = client.list_routes().await.unwrap();
    assert!(routes.is_empty());

    // 2. Add route
    let added = client
        .add_route(
            "api.test.com",
            Some("127.0.0.1:8080".parse().unwrap()),
            Some("vm-02".to_string()),
        )
        .await
        .unwrap();
    assert!(added);

    // Verify in registry directly
    let found = registry.lookup("api.test.com");
    assert!(found.is_some());
    let route = found.unwrap();
    assert_eq!(route.node.node_id, "vm-02");
    assert_eq!(route.upstream, Some("127.0.0.1:8080".parse().unwrap()));

    // 3. List routes via IPC
    let list = client.list_routes().await.unwrap();
    assert_eq!(list.len(), 1);
    let info = list.get("api.test.com").unwrap();
    assert_eq!(info.node_id, "vm-02");
    assert_eq!(info.upstream, Some("127.0.0.1:8080".parse().unwrap()));

    // 4. Remove route via IPC
    let removed = client.remove_route("api.test.com").await.unwrap();
    assert!(removed);
    assert!(registry.lookup("api.test.com").is_none());

    // 5. Removing non-existent route returns false
    let removed_again = client.remove_route("api.test.com").await.unwrap();
    assert!(!removed_again);

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
}

#[tokio::test]
async fn test_ipc_multiple_concurrent_clients() {
    let socket = temp_socket_path("concurrent");
    let registry = Arc::new(DomainRegistry::new());
    let server = IpcServer::new(&socket, registry, ProxyMode::Direct);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut handles = Vec::new();
    for i in 0..10 {
        let sock = socket.clone();
        handles.push(tokio::spawn(async move {
            let mut client = IpcClient::connect(&sock).await.unwrap();
            let pong = client.ping().await.unwrap();
            assert_eq!(pong, "pong");

            let domain = format!("worker-{i}.example.com");
            let added = client
                .add_route(
                    &domain,
                    Some(format!("127.0.0.1:{}", 3000 + i).parse().unwrap()),
                    Some("self".to_string()),
                )
                .await
                .unwrap();
            assert!(added);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Verify all 10 were registered
    let mut check_client = IpcClient::connect(&socket).await.unwrap();
    let routes = check_client.list_routes().await.unwrap();
    assert_eq!(routes.len(), 10);

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
}

#[tokio::test]
async fn test_ipc_health_and_proxy_status() {
    let socket = temp_socket_path("health");
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        "api.example.com".to_string(),
        Route::new(
            Some("127.0.0.1:4000".parse().unwrap()),
            Node::new("node-01", "127.0.0.1:4000".parse().unwrap()),
        ),
    );
    let registry = Arc::new(DomainRegistry::with_routes(routes));
    let server = IpcServer::new(&socket, registry, ProxyMode::Direct);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket).await.unwrap();

    // Health check
    let health = client.health().await.unwrap();
    match health {
        IpcData::HealthSummary {
            system_health,
            total_routes,
            healthy_routes,
            ..
        } => {
            assert_eq!(system_health, "healthy");
            assert_eq!(total_routes, 1);
            assert_eq!(healthy_routes, 1);
        }
        other => panic!("expected HealthSummary, got {other:?}"),
    }

    // Proxy status check
    let proxy_status = client.proxy_status().await.unwrap();
    match proxy_status {
        IpcData::ProxyStatus {
            mode,
            routes_count,
            healthy_routes,
            ..
        } => {
            assert_eq!(mode, "Direct");
            assert_eq!(routes_count, 1);
            assert_eq!(healthy_routes, 1);
        }
        other => panic!("expected ProxyStatus, got {other:?}"),
    }

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
}

#[tokio::test]
async fn test_ipc_config_view_and_set() {
    let socket = temp_socket_path("config");
    let registry = Arc::new(DomainRegistry::new());
    let mut server = IpcServer::new(&socket, registry, ProxyMode::Direct);

    // Create a temporary config file for the test
    let tmp_config = temp_socket_path("cfg-test").with_extension("toml");
    std::fs::write(
        &tmp_config,
        r#"
enable_telemetry = false

[proxy]
mode = "Direct"

[logger]
level = "INFO"
"#,
    )
    .unwrap();

    let json_slot = Arc::new(tokio::sync::RwLock::new(None));
    let path_slot = Arc::new(tokio::sync::RwLock::new(Some(tmp_config.clone())));
    server.set_config_slots(json_slot, path_slot);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let srv_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket).await.unwrap();

    // 1. Get config
    let cfg_resp = client.get_config().await.unwrap();
    match cfg_resp {
        IpcData::ConfigView { config, path } => {
            assert!(path.is_some());
            assert_eq!(config["proxy"]["mode"], "Direct");
            assert_eq!(config["logger"]["level"], "INFO");
        }
        other => panic!("expected ConfigView, got {other:?}"),
    }

    // 2. Set config key
    let set_msg = client
        .set_config_key("logger.level", "DEBUG")
        .await
        .unwrap();
    assert!(set_msg.contains("Configuration updated"));

    // 3. Verify in updated config view
    let cfg_updated = client.get_config().await.unwrap();
    match cfg_updated {
        IpcData::ConfigView { config, .. } => {
            assert_eq!(config["logger"]["level"], "DEBUG");
        }
        other => panic!("expected ConfigView, got {other:?}"),
    }

    // 4. Reload config
    let reload_msg = client.reload_config().await.unwrap();
    assert!(reload_msg.contains("Successfully reloaded"));

    let _ = shutdown_tx.send(());
    let _ = srv_handle.await;
    let _ = std::fs::remove_file(&tmp_config);
}
