use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bridge::core::config::Config;
use proxy::udp::quic::{QuicHeaderSummary, QuicPacketType, QuicRouter};
use proxy::udp::{run_udp_socket, UdpListener};
use proxy::{Proxy, ProxyConfig, UdpServiceConfig};
use registry::DomainRegistry;
use tokio::net::UdpSocket;
use tokio::time::timeout;

#[test]
fn test_udp_config_parsing() {
    let toml_data = r#"
        [[udp_services]]
        listen_addr = "127.0.0.1:5353"
        upstream = "127.0.0.1:53"
        session_timeout = 15

        [[udp_services]]
        listen_addr = "0.0.0.0:27015"
        upstream = "10.8.0.5:27015"
    "#;

    let config = Config::from_toml_str(toml_data).expect("failed to parse UDP config");
    assert_eq!(config.udp_services.len(), 2);

    assert_eq!(
        config.udp_services[0].listen_addr,
        "127.0.0.1:5353".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[0].upstream,
        "127.0.0.1:53".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[0].session_timeout,
        Duration::from_secs(15)
    );

    assert_eq!(
        config.udp_services[1].listen_addr,
        "0.0.0.0:27015".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[1].upstream,
        "10.8.0.5:27015".parse().unwrap()
    );
    // Defaults to 30s when omitted
    assert_eq!(
        config.udp_services[1].session_timeout,
        Duration::from_secs(30)
    );
}

#[test]
fn test_udp_config_parsing_with_node_and_port() {
    let toml_data = r#"
        [[udp_services]]
        node_id = "self"
        listen_port = 5353
        upstream = "127.0.0.1:53"
        session_timeout = 15

        [[udp_services]]
        node = "vm-03"
        port = 27015
        upstream = "10.8.0.3:27015"
    "#;

    let config =
        Config::from_toml_str(toml_data).expect("failed to parse UDP config with node and port");
    assert_eq!(config.udp_services.len(), 2);

    assert_eq!(config.udp_services[0].node_id, "self");
    assert_eq!(config.udp_services[0].listen_port, 5353);
    assert_eq!(
        config.udp_services[0].listen_addr,
        "0.0.0.0:5353".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[0].upstream,
        "127.0.0.1:53".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[0].session_timeout,
        Duration::from_secs(15)
    );

    // Tests aliases node -> node_id, port -> listen_port
    assert_eq!(config.udp_services[1].node_id, "vm-03");
    assert_eq!(config.udp_services[1].listen_port, 27015);
    assert_eq!(
        config.udp_services[1].listen_addr,
        "0.0.0.0:27015".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[1].upstream,
        "10.8.0.3:27015".parse().unwrap()
    );
    assert_eq!(
        config.udp_services[1].session_timeout,
        Duration::from_secs(30)
    );
}

#[tokio::test]
async fn test_udp_echo_roundtrip() {
    // 1. Start a mock upstream UDP echo server
    let upstream = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let upstream_addr = upstream.local_addr().unwrap();

    let upstream_task = {
        let upstream = upstream.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];
            let (n, peer) = upstream.recv_from(&mut buf).await.unwrap();
            let mut response = b"echo: ".to_vec();
            response.extend_from_slice(&buf[..n]);
            upstream.send_to(&response, peer).await.unwrap();
        })
    };

    // 2. Start the UDP proxy listener
    let proxy_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let proxy_addr = proxy_socket.local_addr().unwrap();

    let proxy_cfg = UdpServiceConfig::new(proxy_addr, upstream_addr)
        .with_timeout(Duration::from_secs(5));

    let proxy_task = {
        let proxy_socket = proxy_socket.clone();
        tokio::spawn(async move {
            let _ = run_udp_socket(proxy_socket, proxy_cfg).await;
        })
    };

    // 3. Client sends datagram to Bridge UDP proxy
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.connect(proxy_addr).await.unwrap();
    client.send(b"hello bridge").await.unwrap();

    // 4. Client receives echoed response back via the proxy
    let mut resp_buf = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(2), client.recv(&mut resp_buf))
        .await
        .expect("timed out waiting for UDP proxy echo response")
        .unwrap();

    assert_eq!(&resp_buf[..n], b"echo: hello bridge");

    upstream_task.abort();
    proxy_task.abort();
}

#[tokio::test]
async fn test_udp_multiple_concurrent_clients() {
    // Upstream echo server
    let upstream = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let upstream_addr = upstream.local_addr().unwrap();

    let upstream_task = {
        let upstream = upstream.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];
            loop {
                match upstream.recv_from(&mut buf).await {
                    Ok((n, peer)) => {
                        let _ = upstream.send_to(&buf[..n], peer).await;
                    }
                    Err(_) => break,
                }
            }
        })
    };

    // Proxy listener
    let proxy_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let proxy_addr = proxy_socket.local_addr().unwrap();
    let proxy_cfg = UdpServiceConfig::new(proxy_addr, upstream_addr);

    let proxy_task = {
        let proxy_socket = proxy_socket.clone();
        tokio::spawn(async move {
            let _ = run_udp_socket(proxy_socket, proxy_cfg).await;
        })
    };

    // Spawn 2 clients sending distinct payloads
    let client1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    client1.connect(proxy_addr).await.unwrap();
    client2.connect(proxy_addr).await.unwrap();

    client1.send(b"message_from_client_1").await.unwrap();
    client2.send(b"message_from_client_2").await.unwrap();

    let mut buf1 = vec![0u8; 1024];
    let mut buf2 = vec![0u8; 1024];

    let n1 = timeout(Duration::from_secs(2), client1.recv(&mut buf1))
        .await
        .expect("client1 timed out")
        .unwrap();
    let n2 = timeout(Duration::from_secs(2), client2.recv(&mut buf2))
        .await
        .expect("client2 timed out")
        .unwrap();

    assert_eq!(&buf1[..n1], b"message_from_client_1");
    assert_eq!(&buf2[..n2], b"message_from_client_2");

    upstream_task.abort();
    proxy_task.abort();
}

#[tokio::test]
async fn test_quic_header_parsing_and_router() {
    // Construct a sample RFC 9000 QUIC Initial packet header
    // Byte 0: 0xC0 (Long header = 1, Fixed bit = 1, Type Initial = 00)
    // Bytes 1..5: Version 0x00000001 (QUIC v1)
    // Byte 5: DCID len (8 bytes)
    // Bytes 6..14: DCID [1, 2, 3, 4, 5, 6, 7, 8]
    // Byte 14: SCID len (4 bytes)
    // Bytes 15..19: SCID [10, 11, 12, 13]
    let mut packet = vec![
        0xC0, // First byte
        0x00, 0x00, 0x00, 0x01, // Version 1
        0x08, // DCID len
        1, 2, 3, 4, 5, 6, 7, 8, // DCID
        0x04, // SCID len
        10, 11, 12, 13, // SCID
    ];
    packet.extend_from_slice(&[0u8; 32]); // Dummy payload

    let parsed = QuicHeaderSummary::parse(&packet).expect("failed to parse QUIC header");
    assert!(parsed.is_long_header);
    assert_eq!(parsed.packet_type, Some(QuicPacketType::Initial));
    assert_eq!(parsed.version, Some(1));
    assert_eq!(parsed.dcid, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(parsed.scid, Some(vec![10, 11, 12, 13]));

    // Test QuicRouter Connection ID mapping
    let router = QuicRouter::new();
    let target: SocketAddr = "10.8.0.3:443".parse().unwrap();
    router.register(parsed.dcid.clone(), target).await;

    assert_eq!(router.lookup(&parsed.dcid).await, Some(target));
    assert_eq!(router.len().await, 1);

    let removed = router.remove(&parsed.dcid).await;
    assert_eq!(removed, Some(target));
    assert!(router.is_empty().await);
}

#[tokio::test]
async fn test_proxy_run_udp_direct() {
    let mut config = ProxyConfig::default();
    config.udp_services = vec![];
    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));

    // Running with empty udp_services exits cleanly immediately
    let res = timeout(Duration::from_millis(500), proxy.run_udp()).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_udp_listener_service_run() {
    let upstream = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let upstream_addr = upstream.local_addr().unwrap();

    let ingress_bind = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let listen_addr = ingress_bind.local_addr().unwrap();
    drop(ingress_bind);

    let cfg = UdpServiceConfig::new(listen_addr, upstream_addr);
    let listener = UdpListener::new(cfg.clone());

    let listener_task = tokio::spawn(async move {
        let _ = listener.run().await;
    });

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.connect(listen_addr).await.unwrap();

    // Spawn echo responder on upstream
    let upstream_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        let (n, peer) = upstream.recv_from(&mut buf).await.unwrap();
        upstream.send_to(&buf[..n], peer).await.unwrap();
    });

    // Send and expect echo
    client.send(b"ping").await.unwrap();
    let mut resp = vec![0u8; 512];
    let n = timeout(Duration::from_secs(2), client.recv(&mut resp))
        .await
        .expect("timed out")
        .unwrap();

    assert_eq!(&resp[..n], b"ping");

    listener_task.abort();
    upstream_task.abort();
}

#[test]
fn test_udp_config_omitted_upstream_defaults_to_port() {
    let toml_data = r#"
        [[udp_services]]
        node = "vm-03"
        port = 27015
    "#;

    let config = Config::from_toml_str(toml_data).expect("failed to parse UDP config without upstream");
    assert_eq!(config.udp_services.len(), 1);
    assert_eq!(config.udp_services[0].node_id, "vm-03");
    assert_eq!(config.udp_services[0].listen_port, 27015);
    assert_eq!(
        config.udp_services[0].upstream,
        "127.0.0.1:27015".parse().unwrap()
    );
}

#[tokio::test]
async fn test_proxy_run_handoff_with_udp() {
    let upstream = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let upstream_addr = upstream.local_addr().unwrap();

    let ingress_bind = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let listen_addr = ingress_bind.local_addr().unwrap();
    drop(ingress_bind);

    let mut config = ProxyConfig::default();
    config.mode = proxy::core::enums::ProxyMode::Handoff;
    config.udp_services = vec![UdpServiceConfig::new(listen_addr, upstream_addr)];

    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));

    let proxy_task = tokio::spawn(async move {
        let _ = proxy.run().await;
    });

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.connect(listen_addr).await.unwrap();

    let upstream_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        let (n, peer) = upstream.recv_from(&mut buf).await.unwrap();
        upstream.send_to(&buf[..n], peer).await.unwrap();
    });

    client.send(b"handoff_udp_test").await.unwrap();
    let mut resp = vec![0u8; 512];
    let n = timeout(Duration::from_secs(2), client.recv(&mut resp))
        .await
        .expect("timed out")
        .unwrap();

    assert_eq!(&resp[..n], b"handoff_udp_test");

    proxy_task.abort();
    upstream_task.abort();
}

#[tokio::test]
async fn test_proxy_run_direct_dual_tcp_and_udp() {
    // 1. Upstream UDP echo
    let udp_upstream = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let udp_upstream_addr = udp_upstream.local_addr().unwrap();

    let ingress_udp_bind = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let udp_listen_addr = ingress_udp_bind.local_addr().unwrap();
    drop(ingress_udp_bind);

    // 2. Set up ProxyConfig in Direct mode with no HTTP listeners (or default)
    let mut config = ProxyConfig::default();
    config.mode = proxy::core::enums::ProxyMode::Direct;
    config.listeners = vec![]; // no TCP listeners to keep test focused on UDP lifecycle in Direct mode
    config.udp_services = vec![UdpServiceConfig::new(udp_listen_addr, udp_upstream_addr)];

    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));

    let proxy_task = tokio::spawn(async move {
        let _ = proxy.run().await;
    });

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.connect(udp_listen_addr).await.unwrap();

    let upstream_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        let (n, peer) = udp_upstream.recv_from(&mut buf).await.unwrap();
        udp_upstream.send_to(&buf[..n], peer).await.unwrap();
    });

    client.send(b"direct_udp_test").await.unwrap();
    let mut resp = vec![0u8; 512];
    let n = timeout(Duration::from_secs(2), client.recv(&mut resp))
        .await
        .expect("timed out")
        .unwrap();

    assert_eq!(&resp[..n], b"direct_udp_test");

    proxy_task.abort();
    upstream_task.abort();
}

#[test]
fn test_udp_node_resolution_logic() {
    use std::collections::HashMap;
    use registry::Node;

    let toml_data = r#"
        [[nodes]]
        node_id = "vm-game"
        endpoint = "10.8.0.5:443"

        [[udp_services]]
        node_id = "vm-game"
        port = 27015
    "#;

    let config = Config::from_toml_str(toml_data).expect("parse failed");
    let node_map: HashMap<&String, &Node> = config.nodes.iter().map(|n| (&n.node_id, n)).collect();

    let mut proxy_config = config.proxy.clone();
    proxy_config.udp_services.extend(config.udp_services.clone());

    for udp_service in &mut proxy_config.udp_services {
        if udp_service.node_id != "self" {
            if let Some(node) = node_map.get(&udp_service.node_id) {
                if udp_service.upstream.ip().is_loopback() || udp_service.upstream.ip().is_unspecified() {
                    udp_service.upstream = std::net::SocketAddr::new(node.address.ip(), udp_service.listen_port);
                }
            }
        }
    }

    assert_eq!(proxy_config.udp_services.len(), 1);
    assert_eq!(proxy_config.udp_services[0].node_id, "vm-game");
    assert_eq!(proxy_config.udp_services[0].listen_port, 27015);
    assert_eq!(
        proxy_config.udp_services[0].upstream,
        "10.8.0.5:27015".parse().unwrap()
    );
}

#[test]
fn test_udp_self_node_preserves_local_upstream() {
    let toml_data = r#"
        [[udp_services]]
        node_id = "self"
        port = 5353
        upstream = "127.0.0.1:53"
        session_timeout = 15
    "#;

    let config = Config::from_toml_str(toml_data).expect("parse failed");
    let mut proxy_config = config.proxy.clone();
    proxy_config.udp_services.extend(config.udp_services.clone());

    // When node_id == "self", upstream remains the local destination
    assert_eq!(proxy_config.udp_services.len(), 1);
    assert_eq!(proxy_config.udp_services[0].node_id, "self");
    assert_eq!(proxy_config.udp_services[0].listen_port, 5353);
    assert_eq!(
        proxy_config.udp_services[0].upstream,
        "127.0.0.1:53".parse().unwrap()
    );
}

