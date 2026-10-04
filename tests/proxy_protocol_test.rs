use std::net::SocketAddr;
use std::sync::Arc;

use bridge::core::config::Config;
use proxy::core::config::ProxyConfig;
use proxy::core::enums::{ProxyMode, Scheme};
use proxy::handoff::{
    encode_proxy_v2, parse_proxy_v2, parse_tls_sni, ProxyCommand, TransportProtocol,
    PROXY_V2_PREFIX,
};
use proxy::Proxy;
use registry::{DomainRegistry, Node, Route};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Helper to build a valid RFC-compliant TLS ClientHello containing an SNI extension.
fn build_tls_client_hello(sni_hostname: &str) -> Vec<u8> {
    let host_bytes = sni_hostname.as_bytes();
    let host_len = host_bytes.len() as u16;

    let sni_list_len = 1 + 2 + host_len;
    let mut sni_ext_data = Vec::new();
    sni_ext_data.extend_from_slice(&sni_list_len.to_be_bytes());
    sni_ext_data.push(0x00); // host_name
    sni_ext_data.extend_from_slice(&host_len.to_be_bytes());
    sni_ext_data.extend_from_slice(host_bytes);

    let mut extensions = Vec::new();
    extensions.extend_from_slice(&0x0000u16.to_be_bytes());
    extensions.extend_from_slice(&(sni_ext_data.len() as u16).to_be_bytes());
    extensions.extend_from_slice(&sni_ext_data);

    let mut ext_block = Vec::new();
    ext_block.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    ext_block.extend_from_slice(&extensions);

    let mut ch_body = Vec::new();
    ch_body.extend_from_slice(&[0x03, 0x03]); // TLS 1.2
    ch_body.extend_from_slice(&[0x42; 32]);   // 32 random bytes
    ch_body.push(0x00);                      // Session ID len
    ch_body.extend_from_slice(&2u16.to_be_bytes());
    ch_body.extend_from_slice(&[0xc0, 0x2f]);
    ch_body.push(0x01);
    ch_body.push(0x00);
    ch_body.extend_from_slice(&ext_block);

    let ch_len = ch_body.len();
    let mut handshake = Vec::new();
    handshake.push(0x01); // ClientHello
    handshake.push(0x00);
    handshake.extend_from_slice(&(ch_len as u16).to_be_bytes());
    handshake.extend_from_slice(&ch_body);

    let mut record = Vec::new();
    record.push(0x16); // Handshake
    record.extend_from_slice(&[0x03, 0x01]); // TLS 1.0 record version
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

#[test]
fn test_proxy_v2_ipv4_binary_encoding() {
    let src: SocketAddr = "192.0.2.100:54321".parse().unwrap();
    let dst: SocketAddr = "198.51.100.1:443".parse().unwrap();

    let header = encode_proxy_v2(src, dst);

    assert_eq!(header.len(), 28);
    // 12-byte fixed prefix
    assert_eq!(&header[0..12], &PROXY_V2_PREFIX);
    // Byte 12: Version 2 (0x20) | Command PROXY (0x01) = 0x21
    assert_eq!(header[12], 0x21);
    // Byte 13: AF_INET (0x10) | STREAM (0x01) = 0x11
    assert_eq!(header[13], 0x11);
    // Bytes 14-15: Address length = 12 (0x000C)
    assert_eq!(&header[14..16], &12u16.to_be_bytes());
    // Source IPv4
    assert_eq!(&header[16..20], &[192, 0, 2, 100]);
    // Destination IPv4
    assert_eq!(&header[20..24], &[198, 51, 100, 1]);
    // Source Port
    assert_eq!(&header[24..26], &54321u16.to_be_bytes());
    // Destination Port
    assert_eq!(&header[26..28], &443u16.to_be_bytes());
}

#[test]
fn test_proxy_v2_ipv6_binary_encoding() {
    let src: SocketAddr = "[2001:db8::1]:54321".parse().unwrap();
    let dst: SocketAddr = "[2001:db8::2]:443".parse().unwrap();

    let header = encode_proxy_v2(src, dst);

    assert_eq!(header.len(), 52);
    assert_eq!(&header[0..12], &PROXY_V2_PREFIX);
    // Byte 12: Version 2 | Command PROXY = 0x21
    assert_eq!(header[12], 0x21);
    // Byte 13: AF_INET6 (0x20) | STREAM (0x01) = 0x21
    assert_eq!(header[13], 0x21);
    // Bytes 14-15: Address length = 36 (0x0024)
    assert_eq!(&header[14..16], &36u16.to_be_bytes());
    // Source IPv6
    assert_eq!(&header[16..32], &[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    // Destination IPv6
    assert_eq!(&header[32..48], &[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    // Source Port
    assert_eq!(&header[48..50], &54321u16.to_be_bytes());
    // Destination Port
    assert_eq!(&header[50..52], &443u16.to_be_bytes());
}

#[test]
fn test_proxy_v2_roundtrip_parsing() {
    // 1. IPv4 roundtrip
    let src_v4: SocketAddr = "203.0.113.195:49152".parse().unwrap();
    let dst_v4: SocketAddr = "10.8.0.1:443".parse().unwrap();
    let encoded_v4 = encode_proxy_v2(src_v4, dst_v4);

    let (parsed_v4, consumed_v4) = parse_proxy_v2(&encoded_v4).unwrap().unwrap();
    assert_eq!(consumed_v4, 28);
    assert_eq!(parsed_v4.version, 2);
    assert_eq!(parsed_v4.command, ProxyCommand::Proxy);
    assert_eq!(parsed_v4.transport_protocol, TransportProtocol::Stream);
    assert_eq!(parsed_v4.src_addr, src_v4);
    assert_eq!(parsed_v4.dst_addr, dst_v4);

    // 2. IPv6 roundtrip
    let src_v6: SocketAddr = "[2606:4700:4700::1111]:60000".parse().unwrap();
    let dst_v6: SocketAddr = "[fd00::1]:8443".parse().unwrap();
    let encoded_v6 = encode_proxy_v2(src_v6, dst_v6);

    let (parsed_v6, consumed_v6) = parse_proxy_v2(&encoded_v6).unwrap().unwrap();
    assert_eq!(consumed_v6, 52);
    assert_eq!(parsed_v6.version, 2);
    assert_eq!(parsed_v6.command, ProxyCommand::Proxy);
    assert_eq!(parsed_v6.transport_protocol, TransportProtocol::Stream);
    assert_eq!(parsed_v6.src_addr, src_v6);
    assert_eq!(parsed_v6.dst_addr, dst_v6);
}

#[test]
fn test_proxy_v2_invalid_and_partial_buffers() {
    // Incomplete header (< 16 bytes)
    let partial = vec![0x0D, 0x0A, 0x0D, 0x0A];
    assert_eq!(parse_proxy_v2(&partial).unwrap(), None);

    // Invalid signature
    let bad_sig = vec![0u8; 28];
    assert!(parse_proxy_v2(&bad_sig).is_err());

    // Buffer indicates 12 bytes address, but payload truncated
    let mut header = encode_proxy_v2(
        "127.0.0.1:80".parse().unwrap(),
        "127.0.0.1:443".parse().unwrap(),
    );
    header.truncate(20); // 16 bytes header + only 4 bytes of address
    assert_eq!(parse_proxy_v2(&header).unwrap(), None);
}

#[tokio::test]
async fn test_handoff_with_proxy_protocol_live_forwarding() {
    // 1. Target server receives the connection, reads PROXY protocol v2 header first,
    // verifies the real client IP, then reads the TLS ClientHello and replies.
    let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_addr = target_listener.local_addr().unwrap();

    let target_task = tokio::spawn(async move {
        let (mut stream, _) = target_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        buf.truncate(n);

        // Parse PROXY v2 header from the front of the incoming stream
        let (ppv2_header, consumed) = parse_proxy_v2(&buf)
            .expect("valid proxy v2 header")
            .expect("complete proxy v2 header");

        // Verify the recovered source IP is a loopback client IP (since test runs locally)
        assert!(ppv2_header.src_addr.ip().is_loopback());
        assert_eq!(ppv2_header.command, ProxyCommand::Proxy);

        // The remaining bytes after the PROXY header must be the exact TLS ClientHello!
        let remaining_bytes = &buf[consumed..];
        let sni = parse_tls_sni(remaining_bytes);
        assert_eq!(sni, Some("ppv2-app.example.com".to_string()));

        // Send mock upstream response
        stream.write_all(b"MOCK_UPSTREAM_SUCCESS").await.unwrap();
        stream.flush().await.unwrap();
    });

    // 2. Configure Bridge with global send_proxy_protocol = true
    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge_addr = bridge_listener.local_addr().unwrap();

    let registry = Arc::new(DomainRegistry::new());
    let node = Node::new("vm-coolify", target_addr);
    registry.insert("ppv2-app.example.com".to_string(), Route::new(None, node));

    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        https_addr: bridge_addr,
        listeners: vec![Scheme::Https],
        proxy_protocol: true, // Enable PROXY protocol v2
        ..Default::default()
    };

    let proxy = Proxy::new(Arc::new(proxy_config), registry);
    let proxy_handle = tokio::spawn(async move {
        proxy.run_listeners(vec![bridge_listener]).await.unwrap();
    });

    // 3. Client connects and sends standard TLS ClientHello (without PROXY header)
    let mut client = TcpStream::connect(bridge_addr).await.unwrap();
    let client_hello = build_tls_client_hello("ppv2-app.example.com");
    client.write_all(&client_hello).await.unwrap();
    client.flush().await.unwrap();

    // 4. Client reads response from upstream
    let mut resp = [0u8; 64];
    let n = client.read(&mut resp).await.unwrap();
    assert_eq!(&resp[..n], b"MOCK_UPSTREAM_SUCCESS");

    target_task.await.unwrap();
    proxy_handle.abort();
}

#[tokio::test]
async fn test_handoff_per_node_proxy_protocol_override() {
    // Target 1: expects PROXY protocol (node override = true, while global is false)
    let target1_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target1_addr = target1_listener.local_addr().unwrap();

    // Target 2: expects NO PROXY protocol (raw TLS ClientHello only)
    let target2_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target2_addr = target2_listener.local_addr().unwrap();

    let target1_task = tokio::spawn(async move {
        let (mut stream, _) = target1_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        buf.truncate(n);

        // Must start with PROXY Protocol v2 prefix
        assert!(parse_proxy_v2(&buf).unwrap().is_some());
        stream.write_all(b"OK_TARGET1").await.unwrap();
    });

    let target2_task = tokio::spawn(async move {
        let (mut stream, _) = target2_listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        buf.truncate(n);

        // Must NOT start with PROXY Protocol v2 prefix (first byte must be TLS record 0x16)
        assert_eq!(buf[0], 0x16);
        assert!(parse_proxy_v2(&buf).is_err());
        stream.write_all(b"OK_TARGET2").await.unwrap();
    });

    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge_addr = bridge_listener.local_addr().unwrap();

    let registry = Arc::new(DomainRegistry::new());
    // Node 1: explicit with_proxy_protocol(true)
    let node1 = Node::new("vm-ppv2", target1_addr).with_proxy_protocol(true);
    // Node 2: explicit with_proxy_protocol(false) or omitted
    let node2 = Node::new("vm-plain", target2_addr).with_proxy_protocol(false);

    registry.insert("target1.test".to_string(), Route::new(None, node1));
    registry.insert("target2.test".to_string(), Route::new(None, node2));

    // Global proxy_protocol is false!
    let proxy_config = ProxyConfig {
        mode: ProxyMode::Handoff,
        https_addr: bridge_addr,
        listeners: vec![Scheme::Https],
        proxy_protocol: false,
        ..Default::default()
    };

    let proxy = Proxy::new(Arc::new(proxy_config), registry);
    let proxy_handle = tokio::spawn(async move {
        proxy.run_listeners(vec![bridge_listener]).await.unwrap();
    });

    // Request 1 -> target1.test (gets PROXY header)
    let mut client1 = TcpStream::connect(bridge_addr).await.unwrap();
    client1.write_all(&build_tls_client_hello("target1.test")).await.unwrap();
    let mut resp1 = [0u8; 32];
    let n = client1.read(&mut resp1).await.unwrap();
    assert_eq!(&resp1[..n], b"OK_TARGET1");

    // Request 2 -> target2.test (no PROXY header)
    let mut client2 = TcpStream::connect(bridge_addr).await.unwrap();
    client2.write_all(&build_tls_client_hello("target2.test")).await.unwrap();
    let mut resp2 = [0u8; 32];
    let n = client2.read(&mut resp2).await.unwrap();
    assert_eq!(&resp2[..n], b"OK_TARGET2");

    target1_task.await.unwrap();
    target2_task.await.unwrap();
    proxy_handle.abort();
}

#[test]
fn test_config_proxy_protocol_deserialization_yaml_and_toml() {
    let yaml = r#"
proxy:
  mode: handoff
  proxy_protocol: true
nodes:
  - node_id: vm-coolify
    endpoint: 10.8.0.3:443
    proxy_protocol: true
  - node_id: vm-direct
    endpoint: 10.8.0.4:443
    proxy_protocol: false
"#;

    let cfg = Config::from_yaml_str(yaml).unwrap();
    assert!(cfg.proxy.proxy_protocol);
    assert_eq!(cfg.nodes.len(), 2);
    assert_eq!(cfg.nodes[0].proxy_protocol, Some(true));
    assert_eq!(cfg.nodes[1].proxy_protocol, Some(false));

    let toml = r#"
[proxy]
mode = "handoff"
send_proxy_protocol = true

[[nodes]]
node_id = "vm-coolify"
endpoint = "10.8.0.3:443"
send_proxy_protocol = true
"#;

    let cfg_toml = Config::from_toml_str(toml).unwrap();
    assert!(cfg_toml.proxy.proxy_protocol);
    assert_eq!(cfg_toml.nodes[0].proxy_protocol, Some(true));
}
