use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bridge::registry::{
    HashRing, Node, Route, RoutingPreference, DEFAULT_VNODES_PER_NODE,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn test_hash_ring_vnode_generation_and_order() {
    let mut ring = HashRing::new();
    assert_eq!(ring.vnodes_per_node(), DEFAULT_VNODES_PER_NODE);
    assert_eq!(DEFAULT_VNODES_PER_NODE, 360);
    assert!(ring.is_empty());

    ring.add_node("node-a".to_string());
    ring.add_node("node-b".to_string());
    ring.add_node("node-c".to_string());

    assert_eq!(ring.node_count(), 3);
    assert_eq!(ring.vnode_count(), 3 * 360); // 1080 tokens

    // Verify tokens on the ring are sorted ascending
    let tokens: Vec<u64> = (0..ring.vnode_count())
        .map(|idx| {
            // Using get_by_token verifies binary search and ordering
            ring.get_by_token(idx as u64).unwrap();
            idx as u64
        })
        .collect();
    assert_eq!(tokens.len(), 1080);
}

#[test]
fn test_session_affinity_deterministic_mapping() {
    let mut ring = HashRing::new();
    ring.add_node("node-alpha");
    ring.add_node("node-beta");
    ring.add_node("node-gamma");

    let client_ip_1: IpAddr = "192.168.1.100".parse().unwrap();
    let host = "app.example.com";

    let chosen_1 = ring.get_for_client(client_ip_1, host).copied();
    assert!(chosen_1.is_some());

    // 100 consecutive lookups for client 1 must hit the EXACT same node every time
    for _ in 0..100 {
        assert_eq!(ring.get_for_client(client_ip_1, host).copied(), chosen_1);
    }

    let client_ip_2: IpAddr = "10.0.5.22".parse().unwrap();
    let chosen_2 = ring.get_for_client(client_ip_2, host).copied();
    assert!(chosen_2.is_some());

    // 100 consecutive lookups for client 2 must hit its chosen node every time
    for _ in 0..100 {
        assert_eq!(ring.get_for_client(client_ip_2, host).copied(), chosen_2);
    }
}

#[test]
fn test_uniform_load_distribution() {
    let mut ring = HashRing::new();
    ring.add_node("srv-1");
    ring.add_node("srv-2");
    ring.add_node("srv-3");

    let total_keys = 12_000;
    let mut counts: HashMap<&str, usize> = HashMap::new();

    for i in 0..total_keys {
        let ip = IpAddr::V4(Ipv4Addr::new(
            10,
            ((i >> 16) & 0xFF) as u8,
            ((i >> 8) & 0xFF) as u8,
            (i & 0xFF) as u8,
        ));
        let selected = ring.get_for_client(ip, "service.domain.internal").unwrap();
        *counts.entry(selected).or_insert(0) += 1;
    }

    assert_eq!(counts.len(), 3);
    let expected = total_keys / 3; // 4000
    for (node, count) in &counts {
        let diff = (*count as f64 - expected as f64).abs();
        let relative_error = diff / expected as f64;
        // With 360 vnodes per node, deviation from perfect uniform balance is typically < 15%
        assert!(
            relative_error < 0.20,
            "Node {node} received {count} keys (expected ~{expected}), relative error: {relative_error:.2}"
        );
    }
}

#[test]
fn test_minimal_key_redistribution_on_node_removal() {
    // 4 nodes initially
    let mut ring = HashRing::new();
    ring.add_node("node-1");
    ring.add_node("node-2");
    ring.add_node("node-3");
    ring.add_node("node-4");

    let num_keys = 8_000;
    let mut original_mappings = Vec::with_capacity(num_keys);

    for i in 0..num_keys {
        let key = format!("client-session-{i}");
        let node = *ring.get(&key).unwrap();
        original_mappings.push((key, node));
    }

    // Remove 1 node (node-4)
    let removed = ring.remove_node("node-4");
    assert!(removed);
    assert_eq!(ring.node_count(), 3);

    let mut unchanged_keys = 0;
    let mut remapped_keys = 0;
    let mut invalid_shift = 0;

    for (key, old_node) in &original_mappings {
        let new_node = *ring.get(key).unwrap();
        if *old_node == "node-4" {
            // Keys that were on node-4 MUST be remapped to one of node-1, 2, or 3
            assert_ne!(new_node, "node-4");
            remapped_keys += 1;
        } else if *old_node == new_node {
            // Keys that were on node-1, 2, or 3 should stay on their exact same node!
            unchanged_keys += 1;
        } else {
            // Consistent hashing guarantees no churn between existing non-removed nodes
            invalid_shift += 1;
        }
    }

    // In a consistent hash ring, removing a node should NEVER cause keys on unaffected nodes
    // to jump between each other.
    assert_eq!(invalid_shift, 0, "Keys unexpectedly shifted between surviving nodes");

    // The fraction of keys remapped should be close to 1/N (1/4 = 25%)
    let remapped_ratio = remapped_keys as f64 / num_keys as f64;
    assert!(
        (remapped_ratio - 0.25).abs() < 0.08,
        "Remapped ratio {remapped_ratio} should be ~0.25 (1/4)"
    );
    assert_eq!(unchanged_keys + remapped_keys, num_keys);
}

#[test]
fn test_route_with_targets_consistent_hashing() {
    let node_a = Node::new("vm-01", "10.8.0.1:443".parse().unwrap())
        .with_direct_address("1.1.1.1:443".parse().unwrap())
        .with_routing(RoutingPreference::Direct);

    let node_b = Node::new("vm-02", "10.8.0.2:443".parse().unwrap())
        .with_direct_address("2.2.2.2:443".parse().unwrap())
        .with_routing(RoutingPreference::Direct);

    let node_c = Node::new("vm-03", "10.8.0.3:443".parse().unwrap())
        .with_direct_address("3.3.3.3:443".parse().unwrap())
        .with_routing(RoutingPreference::Direct);

    let route = Route::with_targets(None, vec![node_a, node_b, node_c]);
    assert_eq!(route.targets.len(), 3);
    assert!(route.ring.is_some());

    let client_1: IpAddr = "192.168.1.1".parse().unwrap();
    let client_2: IpAddr = "192.168.1.2".parse().unwrap();

    let target_node_1 = route.select_node_for_client(client_1, "api.example.com");
    let target_node_2 = route.select_node_for_client(client_2, "api.example.com");

    assert!(["vm-01", "vm-02", "vm-03"].contains(&target_node_1.node_id.as_str()));
    assert!(["vm-01", "vm-02", "vm-03"].contains(&target_node_2.node_id.as_str()));

    let addr_1 = route.target_addr_for_client(client_1, "api.example.com", RoutingPreference::Direct);
    assert_eq!(addr_1, target_node_1.direct_address.unwrap());
}

#[test]
fn test_route_dynamic_add_and_remove_target() {
    let node_a = Node::new("node-a", "10.8.0.1:443".parse().unwrap());
    let mut route = Route::new(None, node_a);

    assert_eq!(route.targets.len(), 1);
    assert!(route.ring.is_none());

    // Adding second target initializes the hash ring
    let node_b = Node::new("node-b", "10.8.0.2:443".parse().unwrap());
    route.add_target(node_b);

    assert_eq!(route.targets.len(), 2);
    assert!(route.ring.is_some());
    assert_eq!(route.ring.as_ref().unwrap().vnode_count(), 2 * 360);

    // Adding third target updates the hash ring
    let node_c = Node::new("node-c", "10.8.0.3:443".parse().unwrap());
    route.add_target(node_c);

    assert_eq!(route.targets.len(), 3);
    assert_eq!(route.ring.as_ref().unwrap().vnode_count(), 3 * 360);

    // Removing a target updates the hash ring
    assert!(route.remove_target("node-b"));
    assert_eq!(route.targets.len(), 2);
    assert_eq!(route.ring.as_ref().unwrap().vnode_count(), 2 * 360);

    // Removing down to 1 target collapses ring back to None (fast path)
    assert!(route.remove_target("node-c"));
    assert_eq!(route.targets.len(), 1);
    assert!(route.ring.is_none());
}

#[tokio::test]
async fn test_proxy_handoff_with_consistent_hashing() {
    // 1. Start two mock target backend TCP listeners (Backend B1 and B2)
    let listener_b1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr_b1 = listener_b1.local_addr().unwrap();

    let listener_b2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr_b2 = listener_b2.local_addr().unwrap();

    let b1_received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let b2_received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let b1_clone = b1_received.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener_b1.accept().await {
                b1_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 128];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            }
        }
    });

    let b2_clone = b2_received.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener_b2.accept().await {
                b2_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 128];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            }
        }
    });

    // 2. Setup domain registry with multi-target route
    let node_1 = Node::new("backend-1", addr_b1);
    let node_2 = Node::new("backend-2", addr_b2);
    let route = Route::with_targets(None, vec![node_1, node_2]);

    let mut routes = HashMap::new();
    routes.insert("cluster.local".to_string(), route);
    let registry = std::sync::Arc::new(bridge::registry::DomainRegistry::with_routes(routes));

    // 3. Find a client IP that consistently maps to B1 and another to B2
    let ip_for_b1 = (1..255)
        .map(|i| IpAddr::V4(Ipv4Addr::new(192, 168, 1, i)))
        .find(|ip| {
            let selected = registry
                .lookup("cluster.local")
                .unwrap()
                .select_node_for_client(*ip, "cluster.local")
                .node_id
                .clone();
            selected == "backend-1"
        })
        .expect("found IP mapping to backend-1");

    let ip_for_b2 = (1..255)
        .map(|i| IpAddr::V4(Ipv4Addr::new(192, 168, 1, i)))
        .find(|ip| {
            let selected = registry
                .lookup("cluster.local")
                .unwrap()
                .select_node_for_client(*ip, "cluster.local")
                .node_id
                .clone();
            selected == "backend-2"
        })
        .expect("found IP mapping to backend-2");

    // 4. Test handoff connection for client mapping to B1
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();

    let reg_clone = registry.clone();
    tokio::spawn(async move {
        if let Ok((client_stream, _)) = proxy_listener.accept().await {
            let client_addr = SocketAddr::new(ip_for_b1, 54321);
            let _ = proxy::handoff::handle_handoff_connection(
                client_stream,
                client_addr,
                reg_clone,
                RoutingPreference::Mesh,
                false,
                false,
            )
            .await;
        }
    });

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nHost: cluster.local\r\n\r\n").await.unwrap();
    let mut resp = [0u8; 128];
    let _ = client.read(&mut resp).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(b1_received.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(b2_received.load(std::sync::atomic::Ordering::SeqCst), 0);

    // 5. Test handoff connection for client mapping to B2
    let proxy_listener_2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr_2 = proxy_listener_2.local_addr().unwrap();

    let reg_clone_2 = registry.clone();
    tokio::spawn(async move {
        if let Ok((client_stream, _)) = proxy_listener_2.accept().await {
            let client_addr = SocketAddr::new(ip_for_b2, 54322);
            let _ = proxy::handoff::handle_handoff_connection(
                client_stream,
                client_addr,
                reg_clone_2,
                RoutingPreference::Mesh,
                false,
                false,
            )
            .await;
        }
    });

    let mut client_2 = tokio::net::TcpStream::connect(proxy_addr_2).await.unwrap();
    client_2.write_all(b"GET / HTTP/1.1\r\nHost: cluster.local\r\n\r\n").await.unwrap();
    let mut resp_2 = [0u8; 128];
    let _ = client_2.read(&mut resp_2).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(b1_received.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(b2_received.load(std::sync::atomic::Ordering::SeqCst), 1);
}
