use std::net::{IpAddr, Ipv4Addr};

use bridge::cluster::{ClusterMessage, PeerNode, MAGIC_BYTES, PROTOCOL_VERSION};
use bridge::mesh::{generate_wireguard_keypair, parse_wireguard_key, WireGuardDevice, WireGuardPeer};

#[test]
fn test_wireguard_keypair_generation_and_parsing() {
    let (priv_key, pub_key) = generate_wireguard_keypair();
    assert!(!priv_key.is_empty());
    assert!(!pub_key.is_empty());

    let parsed_priv = parse_wireguard_key(&priv_key).expect("parse private key");
    let parsed_pub = parse_wireguard_key(&pub_key).expect("parse public key");

    assert_eq!(parsed_priv.len(), 32);
    assert_eq!(parsed_pub.len(), 32);
}

#[test]
fn test_wireguard_device_peer_lifecycle() {
    let (_, pub_key) = generate_wireguard_keypair();
    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, 1));
    let mut device = WireGuardDevice::new("wg0", "privkey", pub_key, 51820, mesh_ip);

    assert_eq!(device.peer_count(), 0);

    let peer = WireGuardPeer::new(
        "peer_pubkey_123",
        Some("65.21.100.2:51820".parse().unwrap()),
        vec!["10.8.0.2/32".to_string()],
    );

    device.add_peer(peer.clone());
    assert_eq!(device.peer_count(), 1);

    let found = device.get_peer("peer_pubkey_123").expect("peer found");
    assert_eq!(found.allowed_ips, vec!["10.8.0.2/32"]);
    assert_eq!(found.persistent_keepalive, 25);

    let removed = device.remove_peer("peer_pubkey_123").expect("removed peer");
    assert_eq!(removed.public_key, "peer_pubkey_123");
    assert_eq!(device.peer_count(), 0);
}

#[test]
fn test_cluster_message_encode_decode_roundtrip() {
    let node = PeerNode::new(
        "vm-01",
        "pubkey_base64_abc",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 1)),
        "127.0.0.1:7946".parse().unwrap(),
    );

    let messages = vec![
        ClusterMessage::JoinRequest { node: node.clone() },
        ClusterMessage::JoinResponse {
            peers: vec![node.clone()],
        },
        ClusterMessage::PeerAnnounce { peer: node.clone() },
        ClusterMessage::Ping {
            seq: 42,
            from_node: "vm-01".to_string(),
        },
        ClusterMessage::Ack {
            seq: 42,
            from_node: "vm-02".to_string(),
        },
        ClusterMessage::PingReq {
            seq: 99,
            target_node: "vm-03".to_string(),
            target_endpoint: "10.8.0.3:7946".parse().unwrap(),
        },
        ClusterMessage::Election {
            from_node: "vm-01".to_string(),
            term: 1,
            priority: 10,
        },
        ClusterMessage::ElectionOk {
            from_node: "vm-02".to_string(),
            term: 1,
        },
        ClusterMessage::Coordinator {
            leader: node.clone(),
            term: 1,
        },
        ClusterMessage::CoordinatorAck {
            from_node: "vm-01".to_string(),
            term: 1,
        },
    ];

    for original in messages {
        let encoded = original.encode().expect("encode message");
        assert_eq!(&encoded[0..4], MAGIC_BYTES);
        assert_eq!(encoded[4], PROTOCOL_VERSION);

        let decoded = ClusterMessage::decode(&encoded).expect("decode message");
        assert_eq!(original, decoded);
    }
}

#[test]
fn test_cluster_message_decode_errors() {
    // 1. Packet too small (< 5 bytes)
    let too_small = vec![b'B', b'R', b'D'];
    assert!(ClusterMessage::decode(&too_small).is_err());

    // 2. Invalid magic bytes
    let bad_magic = vec![b'X', b'Y', b'Z', b'W', PROTOCOL_VERSION, 0x01];
    assert!(ClusterMessage::decode(&bad_magic).is_err());

    // 3. Unsupported version
    let bad_version = vec![b'B', b'R', b'D', b'G', 99, 0x01];
    assert!(ClusterMessage::decode(&bad_version).is_err());
}
