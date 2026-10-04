use std::net::SocketAddr;
use std::sync::Arc;

use proxy::core::config::ProxyConfig;
use proxy::core::enums::{ProxyMode, Scheme};
use proxy::handoff::{parse_http_host, parse_http_uri_path, parse_tls_sni};
use proxy::Proxy;
use registry::{DomainRegistry, Node, Route, RoutingPreference};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Helper to build a valid RFC-compliant TLS ClientHello containing an SNI extension.
fn build_tls_client_hello(sni_hostname: &str) -> Vec<u8> {
    let host_bytes = sni_hostname.as_bytes();
    let host_len = host_bytes.len() as u16;

    // SNI extension payload:
    // 2 bytes list length
    // 1 byte name type (0x00 == host_name)
    // 2 bytes host length
    // host bytes
    let sni_list_len = 1 + 2 + host_len;
    let mut sni_ext_data = Vec::new();
    sni_ext_data.extend_from_slice(&sni_list_len.to_be_bytes());
    sni_ext_data.push(0x00); // host_name
    sni_ext_data.extend_from_slice(&host_len.to_be_bytes());
    sni_ext_data.extend_from_slice(host_bytes);

    // Extension header: 2 bytes type (0x0000 for SNI), 2 bytes length
    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0x0000u16.to_be_bytes());
    extensions.extend_from_slice(&(sni_ext_data.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni_ext_data);

    // Extensions block: 2 bytes total extensions length + extensions
    let mut ext_block = Vec::new();
    ext_block.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    ext_block.extend_from_slice(&extensions);

    // ClientHello body:
    // 2 bytes client version (0x0303 for TLS 1.2)
    // 32 bytes random
    // 1 byte session ID len (0)
    // 2 bytes cipher suites len (2) + 2 bytes cipher suite (0xc02f)
    // 1 byte compression methods len (1) + 1 byte (0x00)
    // ext_block
    let mut ch_body = Vec::new();
    ch_body.extend_from_slice(&[0x03, 0x03]); // Version: TLS 1.2
    ch_body.extend_from_slice(&[0x42; 32]);   // 32 bytes random
    ch_body.push(0x00);                      // Session ID length: 0
    ch_body.extend_from_slice(&2u16.to_be_bytes()); // Cipher suites len
    ch_body.extend_from_slice(&[0xc0, 0x2f]);       // Cipher suite
    ch_body.push(0x01);                      // Compression methods len
    ch_body.push(0x00);                      // null compression
    ch_body.extend_from_slice(&ext_block);

    // Handshake header:
    // 1 byte handshake type (0x01 == ClientHello)
    // 3 bytes handshake length
    let ch_len = ch_body.len();
    let mut handshake = vec![
        0x01,
        ((ch_len >> 16) & 0xff) as u8,
        ((ch_len >> 8) & 0xff) as u8,
        (ch_len & 0xff) as u8,
    ];
    handshake.extend_from_slice(&ch_body);

    // TLS Record header:
    // 1 byte content type (0x16 == Handshake)
    // 2 bytes record version (0x03, 0x01)
    // 2 bytes record length
    let mut record = vec![0x16, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);

    record
}

#[test]
fn test_parse_tls_sni_valid() {
    let packet = build_tls_client_hello("coolify.example.com");
    let sni = parse_tls_sni(&packet);
    assert_eq!(sni, Some("coolify.example.com".to_string()));

    // Test uppercase conversion
    let packet_upper = build_tls_client_hello("APP.PROD.INTERNAL");
    let sni_upper = parse_tls_sni(&packet_upper);
    assert_eq!(sni_upper, Some("app.prod.internal".to_string()));
}

#[test]
fn test_parse_tls_sni_invalid_and_edge_cases() {
    // Empty buffer
    assert_eq!(parse_tls_sni(&[]), None);

    // Too short header (< 5 bytes)
    assert_eq!(parse_tls_sni(&[0x16, 0x03, 0x01]), None);

    // Non-handshake record type (e.g. Application data 0x17)
    assert_eq!(parse_tls_sni(&[0x17, 0x03, 0x01, 0x00, 0x05, 0x01, 0x00, 0x00, 0x01, 0x00]), None);

    // Non-ClientHello handshake (e.g. ServerHello 0x02)
    assert_eq!(parse_tls_sni(&[0x16, 0x03, 0x01, 0x00, 0x05, 0x02, 0x00, 0x00, 0x01, 0x00]), None);

    // Truncated payload
    let mut packet = build_tls_client_hello("app.test");
    packet.truncate(packet.len() - 10);
    assert_eq!(parse_tls_sni(&packet), None);
}

#[test]
fn test_parse_http_host() {
    let req = b"GET /index.html HTTP/1.1\r\nHost: app.coolify.test\r\nUser-Agent: curl\r\n\r\n";
    assert_eq!(parse_http_host(req), Some("app.coolify.test".to_string()));

    // Request with port in Host
    let req_with_port = b"POST /api/v1/deploy HTTP/1.1\r\nHost: coolify.internal:8443\r\n\r\n";
    assert_eq!(parse_http_host(req_with_port), Some("coolify.internal".to_string()));

    // Request with IPv6 Host
    let req_ipv6 = b"GET / HTTP/1.1\r\nHost: [::1]:8080\r\n\r\n";
    assert_eq!(parse_http_host(req_ipv6), Some("::1".to_string()));

    // Non-HTTP data
    assert_eq!(parse_http_host(b"\x16\x03\x01\x00\x10something"), None);

    // HTTP without Host header
    let req_no_host = b"GET / HTTP/1.1\r\nUser-Agent: test\r\n\r\n";
    assert_eq!(parse_http_host(req_no_host), None);
}

#[test]
fn test_parse_http_uri_path() {
    let req = b"GET /dashboard/servers?active=true HTTP/1.1\r\nHost: coolify.test\r\n\r\n";
    assert_eq!(parse_http_uri_path(req), "/dashboard/servers?active=true");

    let req_root = b"GET / HTTP/1.1\r\nHost: coolify.test\r\n\r\n";
    assert_eq!(parse_http_uri_path(req_root), "/");
}

#[test]
fn test_routing_preference_direct_vs_mesh() {
    let direct_addr: SocketAddr = "198.51.100.10:443".parse().unwrap();
    let mesh_addr: SocketAddr = "10.8.0.10:443".parse().unwrap();
    let fallback_addr: SocketAddr = "127.0.0.1:443".parse().unwrap();

    let node = Node::new("vm-01", fallback_addr)
        .with_direct_address(direct_addr)
        .with_mesh_address(mesh_addr);

    // Default node routing (None) with Direct fallback
    assert_eq!(node.target_address_with_fallback(RoutingPreference::Direct), direct_addr);

    // Default node routing (None) with Mesh fallback
    assert_eq!(node.target_address_with_fallback(RoutingPreference::Mesh), mesh_addr);

    // Explicit node routing: Direct
    let node_direct = node.clone().with_routing(RoutingPreference::Direct);
    assert_eq!(node_direct.target_address(), direct_addr);
    assert_eq!(node_direct.target_address_with_fallback(RoutingPreference::Mesh), direct_addr);

    // Explicit node routing: Mesh
    let node_mesh = node.clone().with_routing(RoutingPreference::Mesh);
    assert_eq!(node_mesh.target_address(), mesh_addr);
    assert_eq!(node_mesh.target_address_with_fallback(RoutingPreference::Direct), mesh_addr);

    // Route upstream override takes precedence
    let upstream_addr: SocketAddr = "10.0.0.99:8443".parse().unwrap();
    let route = Route::new(Some(upstream_addr), node_direct);
    assert_eq!(route.target_addr(), upstream_addr);
}

#[test]
fn test_node_toml_deserialization_routing() {
    let toml_str = r#"
        node_id = "vm-01"
        direct_endpoint = "198.51.100.20:443"
        mesh_endpoint = "10.8.0.20:443"
        routing = "direct"
    "#;

    let node: Node = toml::from_str(toml_str).expect("failed to deserialize node");
    assert_eq!(node.node_id, "vm-01");
    assert_eq!(node.routing, Some(RoutingPreference::Direct));
    assert_eq!(node.direct_address, Some("198.51.100.20:443".parse().unwrap()));
    assert_eq!(node.mesh_address, Some("10.8.0.20:443".parse().unwrap()));
    assert_eq!(node.target_address(), "198.51.100.20:443".parse().unwrap());
}

#[tokio::test]
async fn test_handoff_transparent_tls_splicing() {
    // 1. Start a mock target VM server (simulating Coolify/Traefik terminating TLS)
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream_listener.local_addr().unwrap();

    let target_task = tokio::spawn(async move {
        let (mut stream, _) = upstream_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        buf.truncate(n);

        // Verify the mock server received the exact TLS ClientHello with SNI intact
        let sni = parse_tls_sni(&buf);
        assert_eq!(sni, Some("app.mycoolify.test".to_string()));

        // Simulate sending a TLS ServerHello back to the client
        let mock_server_hello = b"MOCK_TLS_SERVER_HELLO_FROM_UPSTREAM";
        stream.write_all(mock_server_hello).await.unwrap();
        stream.flush().await.unwrap();

        // Echo any application data sent after handshake
        let mut app_buf = [0u8; 64];
        let n = stream.read(&mut app_buf).await.unwrap();
        stream.write_all(&app_buf[..n]).await.unwrap();
        stream.flush().await.unwrap();
    });

    // 2. Configure Bridge in Handoff mode
    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge_addr = bridge_listener.local_addr().unwrap();

    let registry = Arc::new(DomainRegistry::new());
    let node = Node::new("vm-01", upstream_addr);
    registry.insert("app.mycoolify.test".to_string(), Route::new(None, node));

    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        https_addr: bridge_addr,
        listeners: vec![Scheme::Https],
        max_concurrency: 4,
        ..Default::default()
    };

    let proxy = Proxy::new(Arc::new(proxy_config), registry);

    // Run Bridge listener in background
    let proxy_handle = tokio::spawn(async move {
        proxy.run_listeners(vec![bridge_listener]).await.unwrap();
    });

    // 3. Client connects to Bridge and sends TLS ClientHello
    let mut client_stream = TcpStream::connect(bridge_addr).await.unwrap();
    let client_hello = build_tls_client_hello("app.mycoolify.test");
    client_stream.write_all(&client_hello).await.unwrap();
    client_stream.flush().await.unwrap();

    // 4. Client reads mock ServerHello from upstream via Bridge passthrough
    let mut response_buf = [0u8; 128];
    let n = client_stream.read(&mut response_buf).await.unwrap();
    assert_eq!(&response_buf[..n], b"MOCK_TLS_SERVER_HELLO_FROM_UPSTREAM");

    // 5. Send application data through spliced stream
    client_stream.write_all(b"APPLICATION_PAYLOAD").await.unwrap();
    client_stream.flush().await.unwrap();

    let n = client_stream.read(&mut response_buf).await.unwrap();
    assert_eq!(&response_buf[..n], b"APPLICATION_PAYLOAD");

    target_task.await.unwrap();
    proxy_handle.abort();
}

#[tokio::test]
async fn test_handoff_direct_vs_mesh_routing() {
    // Spin up two mock target servers: one representing Direct (public IP), one representing Mesh (WireGuard)
    let direct_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let direct_addr = direct_listener.local_addr().unwrap();

    let mesh_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mesh_addr = mesh_listener.local_addr().unwrap();

    let direct_task = tokio::spawn(async move {
        let (mut stream, _) = direct_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let _n = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"RESPONSE_FROM_DIRECT_IP").await.unwrap();
    });

    let mesh_task = tokio::spawn(async move {
        let (mut stream, _) = mesh_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let _n = stream.read(&mut buf).await.unwrap();
        stream.write_all(b"RESPONSE_FROM_MESH_IP").await.unwrap();
    });

    // Node configured with routing = Direct
    let node_direct = Node::new("vm-direct", direct_addr)
        .with_direct_address(direct_addr)
        .with_mesh_address(mesh_addr)
        .with_routing(RoutingPreference::Direct);

    // Node configured with routing = Mesh
    let node_mesh = Node::new("vm-mesh", mesh_addr)
        .with_direct_address(direct_addr)
        .with_mesh_address(mesh_addr)
        .with_routing(RoutingPreference::Mesh);

    let registry = Arc::new(DomainRegistry::new());
    registry.insert("direct.example.com".to_string(), Route::new(None, node_direct));
    registry.insert("mesh.example.com".to_string(), Route::new(None, node_mesh));

    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge_addr = bridge_listener.local_addr().unwrap();

    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        https_addr: bridge_addr,
        listeners: vec![Scheme::Https],
        ..Default::default()
    };

    let proxy = Proxy::new(Arc::new(proxy_config), registry);
    let proxy_handle = tokio::spawn(async move {
        proxy.run_listeners(vec![bridge_listener]).await.unwrap();
    });

    // 1. Connect requesting "direct.example.com"
    let mut client1 = TcpStream::connect(bridge_addr).await.unwrap();
    let hello_direct = build_tls_client_hello("direct.example.com");
    client1.write_all(&hello_direct).await.unwrap();
    let mut resp = [0u8; 64];
    let n = client1.read(&mut resp).await.unwrap();
    assert_eq!(&resp[..n], b"RESPONSE_FROM_DIRECT_IP");

    // 2. Connect requesting "mesh.example.com"
    let mut client2 = TcpStream::connect(bridge_addr).await.unwrap();
    let hello_mesh = build_tls_client_hello("mesh.example.com");
    client2.write_all(&hello_mesh).await.unwrap();
    let mut resp = [0u8; 64];
    let n = client2.read(&mut resp).await.unwrap();
    assert_eq!(&resp[..n], b"RESPONSE_FROM_MESH_IP");

    direct_task.await.unwrap();
    mesh_task.await.unwrap();
    proxy_handle.abort();
}

#[tokio::test]
async fn test_handoff_http_redirect() {
    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge_addr = bridge_listener.local_addr().unwrap();

    let registry = Arc::new(DomainRegistry::new());
    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        http_addr: bridge_addr,
        listeners: vec![Scheme::Http],
        redirect_http: true,
        ..Default::default()
    };

    let proxy = Proxy::new(Arc::new(proxy_config), registry);
    let proxy_handle = tokio::spawn(async move {
        proxy.run_listeners(vec![bridge_listener]).await.unwrap();
    });

    let mut client = TcpStream::connect(bridge_addr).await.unwrap();
    let http_req = b"GET /admin/dashboard?tab=1 HTTP/1.1\r\nHost: coolify.example.com\r\n\r\n";
    client.write_all(http_req).await.unwrap();

    let mut resp = [0u8; 512];
    let n = client.read(&mut resp).await.unwrap();
    let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

    assert!(resp_str.starts_with("HTTP/1.1 301 Moved Permanently"));
    assert!(resp_str.contains("Location: https://coolify.example.com/admin/dashboard?tab=1"));

    proxy_handle.abort();
}
