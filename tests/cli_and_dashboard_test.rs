use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bridge::cluster::{ClusterController, PeerNode};
use bridge::core::config::{FailbackMode, Service, ServiceReplicationConfig};
#[cfg(feature = "dashboard")]
use bridge::dashboard::DashboardServer;
use bridge::failover::{MockContainerDriver, WorkloadDuplicator};
use bridge::ipc::{IpcClient, IpcData, IpcServer};
use bridge::mesh::WireGuardDevice;
use proxy::core::enums::ProxyMode;
use proxy::ShutdownReason;
use registry::{DomainRegistry, Node, Route};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, RwLock};
use url::Url;

/// Helper to create a test `ClusterController`.
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
async fn test_ipc_cluster_status_query() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-ipc-cluster-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());
    let ipc_server = IpcServer::new(&socket_path, registry, ProxyMode::Handoff);

    let (ctrl, local_peer) = create_test_cluster("leader-01", 1).await;
    // Set leader as local node
    ctrl.election_state.write().await.current_leader = Some(local_peer.clone());

    // Add a remote peer
    let remote_peer = PeerNode::new(
        "worker-02",
        "pubkey_worker",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "127.0.0.1:51821".parse().unwrap(),
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

    let cluster_data = client.cluster_status().await.expect("cluster_status");
    match cluster_data {
        IpcData::Cluster {
            local_node_id,
            current_leader,
            is_leader,
            peers,
        } => {
            assert_eq!(local_node_id, "leader-01");
            assert_eq!(current_leader.as_deref(), Some("leader-01"));
            assert!(is_leader);
            assert_eq!(peers.len(), 2);
        }
        other => panic!("unexpected response: {:?}", other),
    }

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_ipc_inspect_domain_with_session_affinity() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-ipc-inspect-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());

    // Create multi-target route with consistent hashing
    let n1 = Node::new("node-1", "127.0.0.1:8081".parse().unwrap());
    let n2 = Node::new("node-2", "127.0.0.1:8082".parse().unwrap());
    let mut route = Route::new(Some("127.0.0.1:8081".parse().unwrap()), n1);
    route.add_target(n2);
    registry.insert("app.example.com".to_string(), route);

    let ipc_server = IpcServer::new(&socket_path, registry, ProxyMode::Handoff);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC socket");

    let client_ip: IpAddr = "192.168.1.100".parse().unwrap();
    let inspect_data = client
        .inspect_domain("app.example.com", Some(client_ip))
        .await
        .expect("inspect_domain");

    match inspect_data {
        IpcData::InspectResult {
            domain,
            total_targets,
            has_hash_ring,
            client_selected_target,
            client_selected_node_id,
            ..
        } => {
            assert_eq!(domain, "app.example.com");
            assert_eq!(total_targets, 2);
            assert!(has_hash_ring);
            assert!(client_selected_target.is_some());
            assert!(client_selected_node_id.is_some());
        }
        other => panic!("unexpected response: {:?}", other),
    }

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn test_ipc_trigger_replication_and_failback() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-ipc-rep-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());
    let (ctrl, local_peer) = create_test_cluster("leader-01", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_peer.clone());

    let mock_driver = Arc::new(MockContainerDriver::new());
    let services = vec![Service {
        url: Url::parse("http://demo.example.com").unwrap(),
        node_id: "worker-02".to_string(),
        upstream: Some("127.0.0.1:8080".parse().unwrap()),
        replicate: Some(ServiceReplicationConfig {
            enabled: true,
            image: Some("demo:v1".to_string()),
            env: vec![],
            container_port: Some(8080),
            placement: "leader".to_string(),
            failback_mode: FailbackMode::Manual,
            failback_cooldown_secs: 30,
        }),
    }];

    let duplicator = Arc::new(WorkloadDuplicator::new(
        local_peer.node_id.clone(),
        services,
        registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    ));

    let ipc_server = IpcServer::new(&socket_path, registry.clone(), ProxyMode::Handoff);
    *ipc_server.cluster_handle().write().await = Some(ctrl);
    *ipc_server.duplicator_handle().write().await = Some(duplicator);

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path)
        .await
        .expect("connect to IPC socket");

    // 1. Initial replicas should be empty
    let initial_replicas = client.list_replicas().await.expect("list_replicas");
    assert_eq!(initial_replicas.len(), 0);

    // 2. Trigger replication for failed node 'worker-02'
    let (spawned_count, msg) = client
        .trigger_replication("worker-02")
        .await
        .expect("trigger_replication");
    assert_eq!(spawned_count, 1);
    assert!(msg.contains("1"));

    // 3. Container should now be running and visible in list_replicas
    assert!(mock_driver.is_running("demo.example.com").await);
    let active_replicas = client.list_replicas().await.expect("list_replicas");
    assert_eq!(active_replicas.len(), 1);
    assert_eq!(active_replicas[0].domain, "demo.example.com");
    assert_eq!(active_replicas[0].origin_node_id, "worker-02");

    // 4. Trigger failback
    let (restored, fb_msg) = client
        .failback("demo.example.com")
        .await
        .expect("failback");
    assert!(restored);
    assert!(fb_msg.contains("restored"));

    // 5. Container should now be stopped
    assert!(!mock_driver.is_running("demo.example.com").await);
    let after_failback = client.list_replicas().await.expect("list_replicas");
    assert_eq!(after_failback.len(), 0);

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(socket_path);
}

#[cfg(feature = "dashboard")]
mod dashboard_tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::sync::mpsc;

#[tokio::test]
async fn test_dashboard_http_server_endpoints() {
    let registry = Arc::new(DomainRegistry::new());
    let (ctrl, local_peer) = create_test_cluster("dash-node", 1).await;
    ctrl.election_state.write().await.current_leader = Some(local_peer.clone());
    ctrl.election_state.write().await.role = bridge::cluster::ElectionRole::Leader;

    let mock_driver = Arc::new(MockContainerDriver::new());
    let duplicator = Arc::new(WorkloadDuplicator::new(
        local_peer.node_id.clone(),
        vec![Service {
            url: Url::parse("http://web.example.com").unwrap(),
            node_id: "dead-vm".to_string(),
            upstream: Some("127.0.0.1:3000".parse().unwrap()),
            replicate: Some(ServiceReplicationConfig {
                enabled: true,
                image: Some("web:v1".to_string()),
                env: vec![],
                container_port: Some(3000),
                placement: "leader".to_string(),
                failback_mode: FailbackMode::Manual,
                failback_cooldown_secs: 10,
            }),
        }],
        registry.clone(),
        mock_driver.clone(),
        ctrl.clone(),
    ));

    let cluster_slot = Arc::new(RwLock::new(Some(ctrl)));
    let duplicator_slot = Arc::new(RwLock::new(Some(duplicator as Arc<dyn bridge::failover::FailoverTrigger>)));

    let listen_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    // Bind temporary listener to get an ephemeral free port
    let tmp_listener = tokio::net::TcpListener::bind(listen_addr).await.unwrap();
    let port = tmp_listener.local_addr().unwrap().port();
    drop(tmp_listener);

    let bound_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let auth = Arc::new(bridge::auth::AuthManager::new(
        &bridge::core::config::AuthConfig::default(),
    ));
    let dashboard = Arc::new(DashboardServer::new(
        bound_addr,
        registry,
        cluster_slot,
        duplicator_slot,
        auth,
    ));

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (ack_tx, mut ack_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        let _ = dashboard.run_with_shutdown(shutdown_rx, Some(ack_tx)).await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let http_client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{port}");

    // 1. GET /healthz
    let healthz_resp = http_client
        .get(format!("{base_url}/healthz"))
        .send()
        .await
        .expect("GET /healthz");
    assert_eq!(healthz_resp.status(), 200);
    let healthz_json: serde_json::Value = healthz_resp.json().await.unwrap();
    assert_eq!(healthz_json["status"], "ok");
    assert!(healthz_json["uptime_secs"].is_number());

    // 2. GET / (UI Dashboard HTML)
    let ui_resp = http_client
        .get(&base_url)
        .send()
        .await
        .expect("GET /");
    assert_eq!(ui_resp.status(), 200);
    let html_body = ui_resp.text().await.unwrap();
    assert!(html_body.contains("BRIDGE"));
    assert!(html_body.contains("Fleet Control"));

    // 3. GET /api/status (JSON)
    let status_resp = http_client
        .get(format!("{base_url}/api/status"))
        .send()
        .await
        .expect("GET /api/status");
    assert_eq!(status_resp.status(), 200);
    let status_json: serde_json::Value = status_resp.json().await.unwrap();
    assert_eq!(status_json["node_id"], "dash-node");
    assert_eq!(status_json["is_leader"], true);

    // 4. POST /api/replicate
    let rep_resp = http_client
        .post(format!("{base_url}/api/replicate"))
        .json(&serde_json::json!({ "node_id": "dead-vm" }))
        .send()
        .await
        .expect("POST /api/replicate");
    assert_eq!(rep_resp.status(), 200);
    let rep_json: serde_json::Value = rep_resp.json().await.unwrap();
    assert_eq!(rep_json["success"], true);
    assert_eq!(rep_json["spawned_count"], 1);

    // 5. POST /api/failback
    let fb_resp = http_client
        .post(format!("{base_url}/api/failback"))
        .json(&serde_json::json!({ "domain": "web.example.com" }))
        .send()
        .await
        .expect("POST /api/failback");
    assert_eq!(fb_resp.status(), 200);
    let fb_json: serde_json::Value = fb_resp.json().await.unwrap();
    assert_eq!(fb_json["success"], true);

    // 6. Graceful shutdown
    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let ack = ack_rx.recv().await.expect("receive shutdown ack");
    assert_eq!(ack.subsystem, "dashboard");
}

/// Spins up a dashboard server on an ephemeral port; returns base URL and shutdown handles.
async fn spawn_dashboard(
    registry: Arc<DomainRegistry>,
) -> (String, broadcast::Sender<ShutdownReason>, mpsc::Receiver<proxy::ShutdownAck>) {
    let cluster_slot = Arc::new(RwLock::new(None));
    let duplicator_slot = Arc::new(RwLock::new(None));
    let auth = Arc::new(bridge::auth::AuthManager::new(
        &bridge::core::config::AuthConfig::default(),
    ));
    let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tmp_listener.local_addr().unwrap().port();
    drop(tmp_listener);

    let dashboard = Arc::new(DashboardServer::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        registry,
        cluster_slot,
        duplicator_slot,
        auth,
    ));
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (ack_tx, ack_rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let _ = dashboard.run_with_shutdown(shutdown_rx, Some(ack_tx)).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (format!("http://127.0.0.1:{port}"), shutdown_tx, ack_rx)
}

#[tokio::test]
async fn test_dashboard_put_config_persists_and_validates() {
    let registry = Arc::new(DomainRegistry::new());
    let (_base_url, shutdown_tx, _ack_rx) = spawn_dashboard(registry).await;

    // Config file on disk that the dashboard "runs from".
    let cfg_path = std::env::temp_dir().join(format!("bridge-put-config-{}.toml", std::process::id()));
    std::fs::write(
        &cfg_path,
        "enable_telemetry = false\n\n[proxy]\nmode = \"Direct\"\nlisteners = [\"http\"]\nhttp_addr = \"127.0.0.1:18080\"\n\n[[nodes]]\nnode_id = \"self\"\nendpoint = \"127.0.0.1:18080\"\n",
    )
    .unwrap();

    // Seed the dashboard with the file's view + path (same as daemon wiring).
    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    let doc: serde_json::Value = toml::from_str(&raw).unwrap();
    let _ = shutdown_tx.send(ShutdownReason::Manual);

    let registry2 = Arc::new(DomainRegistry::new());
    let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tmp_listener.local_addr().unwrap().port();
    drop(tmp_listener);
    let auth2 = Arc::new(bridge::auth::AuthManager::new(
        &bridge::core::config::AuthConfig::default(),
    ));
    let dashboard = Arc::new(DashboardServer::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        registry2,
        Arc::new(RwLock::new(None)),
        Arc::new(RwLock::new(None)),
        auth2,
    ));
    dashboard.set_config(doc.clone(), Some(cfg_path.clone()));
    let (sd_tx, sd_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = dashboard.run_with_shutdown(sd_rx, None).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    // 1. Valid update → 200, persisted to disk, visible via GET.
    let mut updated = doc.clone();
    updated["enable_telemetry"] = serde_json::json!(true);
    let resp = client
        .put(format!("{base}/api/v1/config"))
        .json(&updated)
        .send()
        .await
        .expect("PUT /api/v1/config");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["restart_required"], true);
    let on_disk = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(on_disk.contains("enable_telemetry = true"), "disk content: {on_disk}");
    let get_resp = client
        .get(format!("{base}/api/v1/config"))
        .send()
        .await
        .unwrap();
    let get_body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(get_body["config"]["enable_telemetry"], true);

    // 2. Invalid document (unknown proxy mode) → 422 VALIDATION_ERROR.
    let bad = serde_json::json!({ "proxy": { "mode": "NotAMode" } });
    let resp = client
        .put(format!("{base}/api/v1/config"))
        .json(&bad)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "VALIDATION_ERROR");

    // 3. Malformed JSON body → 400.
    let resp = client
        .put(format!("{base}/api/v1/config"))
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    let _ = sd_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(&cfg_path);
}

#[tokio::test]
async fn test_dashboard_put_config_conflict_without_file() {
    let registry = Arc::new(DomainRegistry::new());
    let (base_url, shutdown_tx, _ack_rx) = spawn_dashboard(registry).await;

    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{base_url}/api/v1/config"))
        .json(&serde_json::json!({ "enable_telemetry": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "CONFLICT");

    let _ = shutdown_tx.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_dashboard_metrics_and_logs_endpoints() {
    let registry = Arc::new(DomainRegistry::new());
    registry.record_request("app.test:80", 12, false);
    registry.record_request("app.test:80", 40, true);
    let (base_url, shutdown_tx, _ack_rx) = spawn_dashboard(registry).await;
    let client = reqwest::Client::new();

    // Metrics reflect recorded proxy traffic (host key has the port stripped).
    let resp = client
        .get(format!("{base_url}/api/v1/metrics"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let route = &body["routes"]["app.test"];
    assert_eq!(route["requests"], 2);
    assert_eq!(route["errors"], 1);
    assert_eq!(route["p50_ms"], 12);
    assert_eq!(route["p95_ms"], 40);

    // Logs endpoint returns the documented shape even with an empty buffer.
    let resp = client
        .get(format!("{base_url}/api/v1/logs?limit=50"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["logs"].is_array());
    assert!(body["total"].is_number());

    let _ = shutdown_tx.send(ShutdownReason::Manual);
}
}

