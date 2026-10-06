use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use bridge::cluster::PeerNode;
use bridge::core::config::{
    resolve_env_str, Config, DnsHandoffConfig, HandoffMode,
};
use bridge::handoff::{DnsManager, DnsUpdateAction};
use proxy::ShutdownReason;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch, Mutex};

/// Helper to start a local mock Cloudflare API server for testing HTTP endpoints.
struct MockCloudflareServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    server_task: tokio::task::JoinHandle<()>,
}

impl MockCloudflareServer {
    async fn start(
        existing_record_ip: Option<&'static str>,
        created_record_id: &'static str,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let req_clone = requests.clone();

        let server_task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };

                let mut buf = [0u8; 4096];
                let n = match socket.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };

                let req_text = String::from_utf8_lossy(&buf[..n]).to_string();
                let first_line = req_text.lines().next().unwrap_or("").to_string();
                req_clone.lock().await.push(first_line.clone());

                let response_body = if first_line.starts_with("GET /zones/") {
                    if let Some(ip) = existing_record_ip {
                        format!(
                            r#"{{"success":true,"errors":[],"messages":[],"result":[{{"id":"rec-existing-123","zone_id":"zone-abc","name":"bridge.example.com","type":"A","content":"{ip}","proxied":false,"ttl":60}}]}}"#
                        )
                    } else {
                        r#"{"success":true,"errors":[],"messages":[],"result":[]}"#.to_string()
                    }
                } else if first_line.starts_with("PATCH /zones/") {
                    r#"{"success":true,"errors":[],"messages":[],"result":{"id":"rec-existing-123","zone_id":"zone-abc","name":"bridge.example.com","type":"A","content":"1.2.3.4","proxied":false,"ttl":60}}"#.to_string()
                } else if first_line.starts_with("POST /zones/") {
                    format!(
                        r#"{{"success":true,"errors":[],"messages":[],"result":{{"id":"{created_record_id}","zone_id":"zone-abc","name":"bridge.example.com","type":"A","content":"1.2.3.4","proxied":false,"ttl":60}}}}"#
                    )
                } else {
                    r#"{"success":false,"errors":[{"code":1000,"message":"Not Found"}],"result":null}"#.to_string()
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );

                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });

        Self {
            base_url,
            requests,
            server_task,
        }
    }
}

impl Drop for MockCloudflareServer {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}

#[test]
fn test_dns_handoff_config_deserialization() {
    let toml_data = r#"
        [handoff]
        mode = "dns"

        [handoff.dns]
        provider = "cloudflare"
        zone_id = "env:CF_ZONE_ID"
        record_name = "bridge.example.com"
        record_id = "rec-custom-999"
        api_token = "env:CF_API_TOKEN"
        ttl = 120
        proxied = true
        target_ip = "192.0.2.1"
        api_base_url = "http://localhost:8080"
    "#;

    let config = Config::from_toml_str(toml_data).expect("parse toml config");
    assert_eq!(config.handoff.mode, HandoffMode::Dns);

    let dns = config.handoff.dns.expect("dns config present");
    assert_eq!(dns.provider, "cloudflare");
    assert_eq!(dns.zone_id.as_deref(), Some("env:CF_ZONE_ID"));
    assert_eq!(dns.record_name.as_deref(), Some("bridge.example.com"));
    assert_eq!(dns.record_id.as_deref(), Some("rec-custom-999"));
    assert_eq!(dns.api_token.as_deref(), Some("env:CF_API_TOKEN"));
    assert_eq!(dns.ttl, 120);
    assert!(dns.proxied);
    assert_eq!(dns.target_ip, Some("192.0.2.1".parse().unwrap()));
    assert_eq!(dns.api_base_url, "http://localhost:8080");
}

#[test]
fn test_dns_handoff_env_resolution() {
    unsafe {
        std::env::set_var("CF_TEST_ZONE_ID", "cf_zone_secret_12345");
        std::env::set_var("CF_TEST_API_TOKEN", "cf_token_secret_abcde");
    }

    assert_eq!(
        resolve_env_str("env:CF_TEST_ZONE_ID"),
        "cf_zone_secret_12345"
    );
    assert_eq!(
        resolve_env_str("env:CF_TEST_API_TOKEN"),
        "cf_token_secret_abcde"
    );
    assert_eq!(resolve_env_str("literal_zone"), "literal_zone");
}

#[tokio::test]
async fn test_dns_manager_update_existing_record_mock_api() {
    let mock = MockCloudflareServer::start(Some("9.9.9.9"), "rec-created-1").await;

    let cfg = DnsHandoffConfig {
        provider: "cloudflare".to_string(),
        zone_id: Some("zone-abc".to_string()),
        record_name: Some("bridge.example.com".to_string()),
        record_id: None,
        api_token: Some("test-token".to_string()),
        ttl: 60,
        proxied: false,
        target_ip: None, // Uses default IP
        api_base_url: mock.base_url.clone(),
    };

    let target_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
    let mgr = DnsManager::new(cfg, "node-1", target_ip);

    let res = mgr.sync_dns().await.expect("sync dns");
    assert_eq!(res.action, DnsUpdateAction::Updated);
    assert_eq!(res.record_id, "rec-existing-123");
    assert_eq!(res.ip, target_ip);

    let reqs = mock.requests.lock().await;
    assert_eq!(reqs.len(), 2);
    assert!(reqs[0].starts_with("GET /zones/zone-abc/dns_records?name=bridge.example.com&type=A"));
    assert!(reqs[1].starts_with("PATCH /zones/zone-abc/dns_records/rec-existing-123"));
}

#[tokio::test]
async fn test_dns_manager_already_matching_record_is_unchanged() {
    // Mock server returns record that already has 1.2.3.4
    let mock = MockCloudflareServer::start(Some("1.2.3.4"), "rec-created-1").await;

    let cfg = DnsHandoffConfig {
        provider: "cloudflare".to_string(),
        zone_id: Some("zone-abc".to_string()),
        record_name: Some("bridge.example.com".to_string()),
        record_id: None,
        api_token: Some("test-token".to_string()),
        ttl: 60,
        proxied: false,
        target_ip: Some(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))),
        api_base_url: mock.base_url.clone(),
    };

    let mgr = DnsManager::new(cfg, "node-1", IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)));

    let res = mgr.sync_dns().await.expect("sync dns");
    assert_eq!(res.action, DnsUpdateAction::Unchanged);
    assert_eq!(res.record_id, "rec-existing-123");

    // Only GET was made, no unnecessary PATCH sent
    let reqs = mock.requests.lock().await;
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].starts_with("GET /zones/zone-abc/dns_records?name=bridge.example.com&type=A"));
}

#[tokio::test]
async fn test_dns_manager_create_record_if_absent_mock_api() {
    // None for existing record returns empty list in GET
    let mock = MockCloudflareServer::start(None, "rec-new-post-888").await;

    let cfg = DnsHandoffConfig {
        provider: "cloudflare".to_string(),
        zone_id: Some("zone-abc".to_string()),
        record_name: Some("bridge.example.com".to_string()),
        record_id: None,
        api_token: Some("test-token".to_string()),
        ttl: 60,
        proxied: false,
        target_ip: None,
        api_base_url: mock.base_url.clone(),
    };

    let target_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
    let mgr = DnsManager::new(cfg, "node-1", target_ip);

    let res = mgr.sync_dns().await.expect("sync dns");
    assert_eq!(res.action, DnsUpdateAction::Created);
    assert_eq!(res.record_id, "rec-new-post-888");

    let reqs = mock.requests.lock().await;
    assert_eq!(reqs.len(), 2);
    assert!(reqs[0].starts_with("GET /zones/zone-abc/dns_records?name=bridge.example.com&type=A"));
    assert!(reqs[1].starts_with("POST /zones/zone-abc/dns_records"));
}

#[tokio::test]
async fn test_dns_manager_explicit_record_id_direct_patch() {
    let mock = MockCloudflareServer::start(None, "unused").await;

    let cfg = DnsHandoffConfig {
        provider: "cloudflare".to_string(),
        zone_id: Some("zone-abc".to_string()),
        record_name: Some("bridge.example.com".to_string()),
        record_id: Some("rec-explicit-555".to_string()),
        api_token: Some("test-token".to_string()),
        ttl: 60,
        proxied: false,
        target_ip: None,
        api_base_url: mock.base_url.clone(),
    };

    let target_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
    let mgr = DnsManager::new(cfg, "node-1", target_ip);

    let res = mgr.sync_dns().await.expect("sync dns");
    assert_eq!(res.action, DnsUpdateAction::Updated);
    assert_eq!(res.record_id, "rec-explicit-555");

    // Only PATCH should be called directly, no GET list needed
    let reqs = mock.requests.lock().await;
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].starts_with("PATCH /zones/zone-abc/dns_records/rec-explicit-555"));
}

#[tokio::test]
async fn test_dns_manager_reactive_leadership_handoff() {
    let mock = MockCloudflareServer::start(Some("9.9.9.9"), "rec-created").await;

    let cfg = DnsHandoffConfig {
        provider: "cloudflare".to_string(),
        zone_id: Some("zone-abc".to_string()),
        record_name: Some("bridge.example.com".to_string()),
        record_id: Some("rec-leader-777".to_string()),
        api_token: Some("test-token".to_string()),
        ttl: 60,
        proxied: false,
        target_ip: None,
        api_base_url: mock.base_url.clone(),
    };

    let target_ip: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
    let manager = Arc::new(DnsManager::new(cfg, "leader-node", target_ip));
    let (leader_tx, leader_rx) = watch::channel::<Option<PeerNode>>(None);
    let (shutdown_tx, shutdown_rx) = broadcast::channel(4);
    let (ack_tx, mut ack_rx) = mpsc::channel(4);

    let mgr_clone = manager.clone();
    tokio::spawn(async move {
        let _ = mgr_clone
            .run_with_shutdown(leader_rx, shutdown_rx, Some(ack_tx))
            .await;
    });

    // 1. Initially no leader -> no DNS update
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let reqs = mock.requests.lock().await;
        assert_eq!(reqs.len(), 0);
    }

    // 2. Another node is elected leader -> no DNS update for leader-node
    let other_peer = PeerNode::new(
        "other-node",
        "pubkey_other",
        "10.8.0.2".parse().unwrap(),
        "10.0.0.2:51820".parse().unwrap(),
    );
    leader_tx.send(Some(other_peer)).unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let reqs = mock.requests.lock().await;
        assert_eq!(reqs.len(), 0);
    }

    // 3. Promote "leader-node" to leader -> triggers Cloudflare DNS update
    let our_peer = PeerNode::new(
        "leader-node",
        "pubkey_ours",
        "10.8.0.1".parse().unwrap(),
        "1.2.3.4:51820".parse().unwrap(),
    );
    leader_tx.send(Some(our_peer)).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    {
        let reqs = mock.requests.lock().await;
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].starts_with("PATCH /zones/zone-abc/dns_records/rec-leader-777"));
    }

    // 4. Trigger shutdown -> manager receives shutdown and sends ack
    shutdown_tx
        .send(ShutdownReason::Manual)
        .unwrap();

    let ack = tokio::time::timeout(Duration::from_secs(1), ack_rx.recv())
        .await
        .expect("shutdown ack within timeout")
        .expect("ack received");
    assert_eq!(ack.subsystem, "dns_handoff");
}
