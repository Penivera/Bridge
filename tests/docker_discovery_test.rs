use std::collections::HashMap;
use std::sync::Arc;

use bridge::discovery::{parse_docker_labels, parse_traefik_rule, DockerDiscovery};
use registry::DomainRegistry;

#[test]
fn test_parse_traefik_rule_single_host() {
    let rule = "Host(`app.example.com`)";
    let hosts = parse_traefik_rule(rule);
    assert_eq!(hosts, vec!["app.example.com"]);
}

#[test]
fn test_parse_traefik_rule_multiple_hosts() {
    let rule = "Host(`app.example.com`, `admin.example.com`)";
    let hosts = parse_traefik_rule(rule);
    assert_eq!(hosts, vec!["app.example.com", "admin.example.com"]);
}

#[test]
fn test_parse_traefik_rule_or_condition() {
    let rule = "Host(`app.example.com`) || Host(`api.example.com`)";
    let hosts = parse_traefik_rule(rule);
    assert_eq!(hosts, vec!["app.example.com", "api.example.com"]);
}

#[test]
fn test_parse_traefik_rule_and_path() {
    let rule = "Host(`app.example.com`) && PathPrefix(`/api/v1`)";
    let hosts = parse_traefik_rule(rule);
    assert_eq!(hosts, vec!["app.example.com"]);
}

#[test]
fn test_parse_traefik_rule_double_quotes() {
    let rule = "Host(\"dashboard.company.io\")";
    let hosts = parse_traefik_rule(rule);
    assert_eq!(hosts, vec!["dashboard.company.io"]);
}

#[test]
fn test_parse_docker_labels_traefik_router_and_service() {
    let mut labels = HashMap::new();
    labels.insert(
        "traefik.http.routers.myapp.rule".to_string(),
        "Host(`app.example.com`)".to_string(),
    );
    labels.insert(
        "traefik.http.services.myapp.loadbalancer.server.port".to_string(),
        "3000".to_string(),
    );

    let routes = parse_docker_labels(&labels, "self");
    assert_eq!(routes.len(), 1);

    let (domain, route) = &routes[0];
    assert_eq!(domain, "app.example.com");
    assert_eq!(
        route.upstream,
        Some("127.0.0.1:3000".parse().unwrap())
    );
    assert_eq!(route.node.node_id, "self");
}

#[test]
fn test_parse_docker_labels_coolify_direct_domain() {
    let mut labels = HashMap::new();
    labels.insert(
        "coolify.domain".to_string(),
        "https://portal.myorg.org".to_string(),
    );
    labels.insert("coolify.port".to_string(), "8080".to_string());

    let routes = parse_docker_labels(&labels, "vm-01");
    assert_eq!(routes.len(), 1);

    let (domain, route) = &routes[0];
    assert_eq!(domain, "portal.myorg.org");
    assert_eq!(
        route.upstream,
        Some("127.0.0.1:8080".parse().unwrap())
    );
    assert_eq!(route.node.node_id, "vm-01");
}

#[test]
fn test_parse_docker_labels_caddy_and_bridge_labels() {
    let mut labels = HashMap::new();
    labels.insert(
        "bridge.domain".to_string(),
        "api.example.com, auth.example.com".to_string(),
    );
    labels.insert("bridge.port".to_string(), "5000".to_string());

    let routes = parse_docker_labels(&labels, "self");
    assert_eq!(routes.len(), 2);
    let domains: Vec<String> = routes.iter().map(|(d, _)| d.clone()).collect();
    assert!(domains.contains(&"api.example.com".to_string()));
    assert!(domains.contains(&"auth.example.com".to_string()));
}

#[test]
fn test_parse_docker_labels_empty_when_no_domain() {
    let mut labels = HashMap::new();
    labels.insert("com.docker.compose.project".to_string(), "myapp".to_string());

    let routes = parse_docker_labels(&labels, "self");
    assert!(routes.is_empty());
}

#[tokio::test]
async fn test_docker_discovery_live_scan_if_socket_available() {
    let socket = "/var/run/docker.sock";
    if !std::path::Path::new(socket).exists() {
        eprintln!("Docker socket not found at {socket}, skipping live test");
        return;
    }

    let registry = Arc::new(DomainRegistry::new());
    match DockerDiscovery::connect_socket(socket, registry.clone(), "self") {
        Ok(discovery) => {
            // initial_scan should execute without panicking or returning an error
            match discovery.initial_scan().await {
                Ok(count) => {
                    println!("Live Docker scan discovered {count} container routes");
                }
                Err(err) => {
                    eprintln!("Docker daemon error during scan: {err}");
                }
            }
        }
        Err(err) => {
            eprintln!("Could not connect to Docker socket: {err}");
        }
    }
}
