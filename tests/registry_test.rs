use registry::{DomainRegistry, Node};
use std::collections::HashMap;
use url::Url;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn entry(node: &str, addr: &str) -> Node {
    Node {
        node_id: node.to_string(),
        endpoint: addr.parse().unwrap(),
    }
}

#[test]
fn test_empty_registry() {
    let reg = DomainRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(reg.lookup(&url("https://app.example.com")).is_none());
}

#[test]
fn test_insert_and_lookup() {
    let reg = DomainRegistry::new();
    let domain = url("https://app.example.com");
    reg.insert(domain.clone(), entry("vm-03", "10.8.0.3:443"));

    let result = reg.lookup(&domain);
    assert!(result.is_some());
    let route = result.unwrap();
    assert_eq!(route.node_id, "vm-03");
    assert_eq!(route.endpoint, "10.8.0.3:443".parse().unwrap());
}

#[test]
fn test_remove() {
    let reg = DomainRegistry::new();
    let domain = url("https://app.example.com");
    reg.insert(domain.clone(), entry("vm-03", "10.8.0.3:443"));
    assert_eq!(reg.len(), 1);

    let removed = reg.remove(&domain);
    assert!(removed.is_some());
    assert!(reg.is_empty());
    assert!(reg.lookup(&domain).is_none());
}

#[test]
fn test_overwrite() {
    let reg = DomainRegistry::new();
    let domain = url("https://app.example.com");
    reg.insert(domain.clone(), entry("vm-03", "10.8.0.3:443"));
    reg.insert(domain.clone(), entry("vm-07", "10.8.0.7:443"));

    let route = reg.lookup(&domain).unwrap();
    assert_eq!(route.node_id, "vm-07");
}

#[test]
fn test_with_routes() {
    let mut routes = HashMap::new();
    let domain1 = url("https://app.example.com");
    let domain2 = url("https://api.example.com");
    routes.insert(domain1.clone(), entry("vm-03", "10.8.0.3:443"));
    routes.insert(domain2.clone(), entry("vm-07", "10.8.0.7:443"));

    let reg = DomainRegistry::with_routes(routes);
    assert_eq!(reg.len(), 2);
    assert!(reg.lookup(&domain1).is_some());
    assert!(reg.lookup(&domain2).is_some());
}

#[test]
fn test_snapshot() {
    let reg = DomainRegistry::new();
    let domain = url("https://app.example.com");
    reg.insert(domain.clone(), entry("vm-03", "10.8.0.3:443"));

    let snap = reg.snapshot();
    assert_eq!(snap.len(), 1);
    assert!(snap.contains_key(&domain));
}
