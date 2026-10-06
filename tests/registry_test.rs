use registry::{DomainRegistry, Node, Route};
use std::collections::HashMap;

fn entry(node: &str, addr: &str) -> Route {
    Route::new(None, Node::new(node, addr.parse().unwrap()))
}

fn entry_with_upstream(upstream: &str, node: &str, addr: &str) -> Route {
    Route::new(
        Some(upstream.parse().unwrap()),
        Node::new(node, addr.parse().unwrap()),
    )
}

#[test]
fn test_empty_registry() {
    let reg = DomainRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.lookup("app.example.com").is_none());
}

#[test]
fn test_insert_and_lookup() {
    let reg = DomainRegistry::new();
    let domain = "app.example.com";
    reg.insert(
        domain.to_string(),
        entry_with_upstream("127.0.0.1:8000", "vm-03", "10.8.0.3:443"),
    );

    let result = reg.lookup(domain);
    assert!(result.is_some());
    let route = result.unwrap();
    assert_eq!(route.node.node_id, "vm-03");
    assert_eq!(
        route.node.address,
        "10.8.0.3:443".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(route.upstream, Some("127.0.0.1:8000".parse().unwrap()));
}

#[test]
fn test_remove() {
    let reg = DomainRegistry::new();
    let domain = "app.example.com";
    reg.insert(domain.to_string(), entry("vm-03", "10.8.0.3:443"));
    assert_eq!(reg.len(), 1);

    let removed = reg.remove(domain);
    assert!(removed.is_some());
    let route = removed.unwrap();
    assert_eq!(route.node.node_id, "vm-03");
    assert!(reg.is_empty());
    assert!(reg.lookup(domain).is_none());
}

#[test]
fn test_overwrite() {
    let reg = DomainRegistry::new();
    let domain = "app.example.com";
    reg.insert(domain.to_string(), entry("vm-03", "10.8.0.3:443"));
    reg.insert(domain.to_string(), entry("vm-07", "10.8.0.7:443"));

    let route = reg.lookup(domain).unwrap();
    assert_eq!(route.node.node_id, "vm-07");
}

#[test]
fn test_with_routes() {
    let mut routes = HashMap::new();
    let domain1 = "app.example.com";
    let domain2 = "api.example.com";
    routes.insert(domain1.to_string(), entry("vm-03", "10.8.0.3:443"));
    routes.insert(domain2.to_string(), entry("vm-07", "10.8.0.7:443"));

    let reg = DomainRegistry::with_routes(routes);
    assert_eq!(reg.len(), 2);
    assert!(reg.lookup(domain1).is_some());
    assert!(reg.lookup(domain2).is_some());
}

#[test]
fn test_snapshot() {
    let reg = DomainRegistry::new();
    let domain = "app.example.com";
    reg.insert(domain.to_string(), entry("vm-03", "10.8.0.3:443"));

    let snap = reg.snapshot();
    assert_eq!(snap.len(), 1);
    assert!(snap.contains_key(domain));
}

#[test]
fn test_insert_versioned_accepts_newer_and_rejects_stale() {
    let reg = DomainRegistry::new();
    let domain = "api.example.com";

    // Initial version 1 on vm-01
    let r1 = entry("vm-01", "10.8.0.1:443").with_clock("vm-01", 1);
    assert!(reg.insert_versioned(domain.to_string(), r1));
    assert_eq!(reg.lookup(domain).unwrap().node.node_id, "vm-01");

    // Stale update version 1 from vm-00 (vm-00 < vm-01 in tie break)
    let r_stale = entry("vm-00", "10.8.0.99:443").with_clock("vm-00", 1);
    assert!(!reg.insert_versioned(domain.to_string(), r_stale));
    assert_eq!(reg.lookup(domain).unwrap().node.node_id, "vm-01");

    // Newer update version 2 from vm-02
    let r2 = entry("vm-02", "10.8.0.2:443").with_clock("vm-02", 2);
    assert!(reg.insert_versioned(domain.to_string(), r2));
    assert_eq!(reg.lookup(domain).unwrap().node.node_id, "vm-02");
}

#[test]
fn test_digest_and_compare_digest_diff() {
    let reg_a = DomainRegistry::new();
    let reg_b = DomainRegistry::new();

    // A has "app.example.com" v2 and "shared.com" v1
    reg_a.insert("app.example.com".to_string(), entry("vm-a", "10.8.0.1:443").with_clock("vm-a", 2));
    reg_a.insert("shared.com".to_string(), entry("vm-a", "10.8.0.1:443").with_clock("vm-a", 1));

    // B has "api.example.com" v1 and "shared.com" v2
    reg_b.insert("api.example.com".to_string(), entry("vm-b", "10.8.0.2:443").with_clock("vm-b", 1));
    reg_b.insert("shared.com".to_string(), entry("vm-b", "10.8.0.2:443").with_clock("vm-b", 2));

    let digest_b = reg_b.digest();
    let (needed_from_b, push_to_b) = reg_a.compare_digest(&digest_b);

    // A needs "api.example.com" (missing on A) and "shared.com" (B is at v2, A is at v1)
    assert!(needed_from_b.contains(&"api.example.com".to_string()));
    assert!(needed_from_b.contains(&"shared.com".to_string()));
    assert_eq!(needed_from_b.len(), 2);

    // A pushes "app.example.com" (missing on B)
    assert!(push_to_b.contains_key("app.example.com"));
    assert_eq!(push_to_b.len(), 1);
}

#[test]
fn test_tombstone_prunes_lookup_but_persists_in_digest() {
    let reg = DomainRegistry::new();
    let domain = "deprecated.example.com";

    let active_route = entry("vm-old", "10.8.0.5:443").with_clock("vm-old", 1);
    reg.insert(domain.to_string(), active_route);
    assert!(reg.lookup(domain).is_some());
    assert_eq!(reg.len(), 1);

    // Replace with tombstone at version 2
    let tombstone_route = entry("vm-old", "10.8.0.5:443").with_clock("vm-old", 2).tombstone();
    assert!(reg.insert_versioned(domain.to_string(), tombstone_route));

    // Lookup returns None and len() is 0
    assert!(reg.lookup(domain).is_none());
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());

    // But digest still carries the tombstone clock to propagate deletion
    let digest = reg.digest();
    assert!(digest.contains_key(domain));
    assert_eq!(digest[domain].version, 2);
}
