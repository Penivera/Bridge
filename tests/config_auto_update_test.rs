use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use bridge::cluster::PeerNode;
use bridge::core::config::Config;

#[test]
fn test_auto_generate_missing_keys_toml() {
    let temp_dir = std::env::temp_dir().join(format!("bridge-cfg-test-keygen-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("bridge.toml");

    let initial_toml = r#"
[node]
id = "worker-01"
endpoint = "172.16.15.14:51821"
"#;
    std::fs::write(&config_path, initial_toml).expect("write initial toml");

    // First run: should generate keys and default ports/IP
    let (changed, pub_key) = Config::auto_generate_missing_keys_and_save(&config_path)
        .expect("auto generate keys");
    assert!(changed);
    assert!(pub_key.is_some());
    let pub_key_str = pub_key.unwrap();

    // Verify file content on disk
    let loaded_config = Config::from_file(&config_path).expect("load config from file");
    let node = loaded_config.node.expect("node config present");
    assert_eq!(node.id, "worker-01");
    assert_eq!(node.listen_port, 51820);
    assert_eq!(node.mesh_ip, IpAddr::V4(Ipv4Addr::new(10, 8, 0, 1)));
    assert!(node.private_key.is_some());
    assert_eq!(node.public_key.as_deref(), Some(pub_key_str.as_str()));

    // Second run: should be idempotent and not change keys
    let (changed_again, _) = Config::auto_generate_missing_keys_and_save(&config_path)
        .expect("auto generate idempotent");
    assert!(!changed_again);

    let reloaded = Config::from_file(&config_path).expect("reload");
    assert_eq!(reloaded.node.unwrap().public_key.as_deref(), Some(pub_key_str.as_str()));
}

#[test]
fn test_persist_peer_to_config_file() {
    let temp_dir = std::env::temp_dir().join(format!("bridge-cfg-test-persist-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("bridge.toml");

    let initial_toml = r#"
[node]
id = "worker-01"
endpoint = "172.16.15.14:51821"
listen_port = 51820
mesh_ip = "10.8.0.2"
"#;
    std::fs::write(&config_path, initial_toml).expect("write initial toml");

    // 1. Persist peer A (leader)
    let peer_a = PeerNode::new(
        "leader-01",
        "leader_pub_key_123=",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 1)),
        "172.16.15.13:51821".parse::<SocketAddr>().unwrap(),
    );

    let updated = Config::persist_peer_to_config_file(&config_path, &peer_a)
        .expect("persist peer a");
    assert!(updated);

    let cfg = Config::from_file(&config_path).expect("load config");
    assert_eq!(cfg.seeds.len(), 1);
    assert_eq!(cfg.seeds[0].id.as_deref(), Some("leader-01"));
    assert_eq!(cfg.seeds[0].public_key.as_deref(), Some("leader_pub_key_123="));
    assert_eq!(cfg.seeds[0].mesh_ip, Some(IpAddr::V4(Ipv4Addr::new(10, 8, 0, 1))));

    // 2. Persist newly discovered peer B
    let peer_b = PeerNode::new(
        "worker-02",
        "worker_02_pub_key_456=",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 3)),
        "172.16.15.15:51821".parse::<SocketAddr>().unwrap(),
    );

    let updated_b = Config::persist_peer_to_config_file(&config_path, &peer_b)
        .expect("persist peer b");
    assert!(updated_b);

    let cfg_b = Config::from_file(&config_path).expect("load config");
    assert_eq!(cfg_b.seeds.len(), 2);
    assert_eq!(cfg_b.seeds[1].id.as_deref(), Some("worker-02"));

    // 3. Persisting local node itself should be ignored
    let peer_self = PeerNode::new(
        "worker-01",
        "my_own_key",
        IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
        "172.16.15.14:51821".parse::<SocketAddr>().unwrap(),
    );
    let updated_self = Config::persist_peer_to_config_file(&config_path, &peer_self)
        .expect("persist peer self");
    assert!(!updated_self);

    let cfg_self = Config::from_file(&config_path).expect("load config");
    assert_eq!(cfg_self.seeds.len(), 2);
}
