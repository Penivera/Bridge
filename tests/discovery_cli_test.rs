use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use bridge::cli::print_discovered_services;
use bridge::ipc::{DiscoveredServiceInfo, IpcClient, IpcServer};
use proxy::core::enums::ProxyMode;
use registry::DomainRegistry;

#[test]
fn test_discovered_service_info_serde() {
    let service = DiscoveredServiceInfo {
        container_id: "c1a2b3c4d5e6".to_string(),
        container_name: "web-server".to_string(),
        image: "nginx:alpine".to_string(),
        domains: vec!["app.example.com".to_string(), "api.example.com".to_string()],
        port: 80,
        upstream: Some("127.0.0.1:80".parse().unwrap()),
        status: "running".to_string(),
    };

    let serialized = serde_json::to_string(&service).expect("serialize DiscoveredServiceInfo");
    let deserialized: DiscoveredServiceInfo =
        serde_json::from_str(&serialized).expect("deserialize DiscoveredServiceInfo");

    assert_eq!(service, deserialized);
}

#[tokio::test]
async fn test_ipc_list_discovery_roundtrip() {
    let tmp_dir = std::env::temp_dir().join(format!("bridge-ipc-disc-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let socket_path = tmp_dir.join("test.sock");

    let registry = Arc::new(DomainRegistry::new());
    let mut server = IpcServer::new(&socket_path, registry, ProxyMode::Direct);

    // Populate mock discovered services
    let services_map = Arc::new(RwLock::new(HashMap::new()));
    let mock_service = DiscoveredServiceInfo {
        container_id: "1234567890ab".to_string(),
        container_name: "test-container".to_string(),
        image: "test-image:latest".to_string(),
        domains: vec!["test.domain.local".to_string()],
        port: 3000,
        upstream: Some(SocketAddr::from(([127, 0, 0, 1], 3000))),
        status: "Up 5 minutes".to_string(),
    };
    services_map
        .write()
        .await
        .insert("1234567890ab".to_string(), mock_service.clone());

    server.set_discovery_handle(services_map);

    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let s_handle = tokio::spawn(async move {
        let _ = server.run(shutdown_rx).await;
    });

    // Wait for socket to bind
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC server");
    let discovered = client
        .list_discovery()
        .await
        .expect("list_discovery from IPC");

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].container_name, "test-container");
    assert_eq!(discovered[0].domains, vec!["test.domain.local".to_string()]);
    assert_eq!(discovered[0].port, 3000);

    let _ = shutdown_tx.send(());
    let _ = s_handle.await;
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_print_discovered_services_formatting() {
    let empty_list: Vec<DiscoveredServiceInfo> = Vec::new();
    // Test table and json output for empty list without panic
    print_discovered_services(&empty_list, false);
    print_discovered_services(&empty_list, true);

    let populated_list = vec![DiscoveredServiceInfo {
        container_id: "a1b2c3d4e5f6".to_string(),
        container_name: "api-backend".to_string(),
        image: "my-service:1.0.0".to_string(),
        domains: vec!["api.example.com".to_string()],
        port: 8080,
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        status: "running".to_string(),
    }];

    // Test table and json output for populated list without panic
    print_discovered_services(&populated_list, false);
    print_discovered_services(&populated_list, true);
}
