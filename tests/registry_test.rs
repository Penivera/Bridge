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
