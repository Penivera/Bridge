use bridge::core::config::{Config, ProxyConfig};
use proxy::core::enums::{ProxyMode, Scheme};

#[test]
fn test_default_config_has_handoff_mode() {
    let config = Config::default();
    assert!(!config.enable_telemetry);
    assert_eq!(config.logger.level, tracing::Level::DEBUG);
    assert_eq!(config.proxy.mode, ProxyMode::Handoff);
    assert_eq!(config.sentry.environment.as_deref(), Some("production"));
    assert_eq!(config.sentry.sample_rate, 1.0);
}

#[test]
fn test_proxy_config_direct() {
    let config = Config {
        proxy: toml::from_str::<ProxyConfig>(
            r#"
            mode = "Direct"
            listeners = ["http", "https"]
            redirect_http = true
            "#,
        )
        .unwrap(),
        ..Default::default()
    };
    assert_eq!(config.proxy.mode, ProxyMode::Direct);
    assert_eq!(config.proxy.listeners.len(), 2);
    assert!(config.proxy.redirect_http);
}

#[test]
fn test_from_toml_str() {
    let toml_data = r#"
        enable_telemetry = true

        [sentry]
        dsn = "https://examplePublicKey@o0.ingest.sentry.io/0"
        environment = "staging"
        sample_rate = 0.5
        traces_sample_rate = 0.1

        [logger]
        level = "INFO"
        format = "json"

        [proxy]
        mode = "Direct"
        listeners = ["Https"]
        redirect_http = false

        [[nodes]]
        node_id = "vm-03"
        endpoint = "10.8.0.3:443"
    "#;

    let config = Config::from_toml_str(toml_data).expect("failed to parse config toml");
    assert!(config.enable_telemetry);
    assert_eq!(
        config.sentry.dsn.as_deref(),
        Some("https://examplePublicKey@o0.ingest.sentry.io/0")
    );
    assert_eq!(config.sentry.environment.as_deref(), Some("staging"));
    assert_eq!(config.sentry.sample_rate, 0.5);
    assert_eq!(config.sentry.traces_sample_rate, 0.1);
    assert_eq!(config.logger.level, tracing::Level::INFO);
    assert_eq!(config.proxy.mode, ProxyMode::Direct);
    assert_eq!(config.proxy.http_addr(), "0.0.0.0:80".parse().unwrap());
    assert_eq!(config.proxy.https_addr(), "0.0.0.0:443".parse().unwrap());
    assert_eq!(config.nodes.len(), 1);
    assert_eq!(config.nodes[0].node_id, "vm-03");
    assert_eq!(config.nodes[0].address, "10.8.0.3:443".parse().unwrap());
}

#[test]
fn test_proxy_config_managed() {
    let config = Config {
        proxy: ProxyConfig {
            mode: ProxyMode::Managed,
            listeners: vec![],
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(config.proxy.mode, ProxyMode::Managed);
    assert!(config.proxy.listeners.is_empty());
}

#[test]
fn test_load_bridge_toml() {
    let config = Config::from_file("bridge.toml").expect("failed to load bridge.toml");
    assert_eq!(config.proxy.mode, ProxyMode::Direct);
    assert_eq!(config.proxy.http_addr(), "0.0.0.0:80".parse().unwrap());
    assert_eq!(config.proxy.https_addr(), "0.0.0.0:443".parse().unwrap());
    assert_eq!(config.http_addr(), "0.0.0.0:80".parse().unwrap());
    assert_eq!(config.https_addr(), "0.0.0.0:443".parse().unwrap());
    assert_eq!(config.nodes.len(), 2);
    assert_eq!(config.nodes[0].node_id, "httpbin-aws");
    assert_eq!(config.nodes[0].address, "3.234.68.252:443".parse().unwrap());
    assert_eq!(config.nodes[1].node_id, "self");
    assert_eq!(config.nodes[1].address, "127.0.0.1:80".parse().unwrap());
    assert_eq!(config.services.len(), 3);
    assert_eq!(config.services[0].url.as_str(), "https://httpbin.org/");
    assert_eq!(config.services[0].node_id, "httpbin-aws");
    assert_eq!(config.services[0].upstream, None);
    assert_eq!(config.services[1].url.as_str(), "http://cafx.test/");
    assert_eq!(config.services[1].node_id, "self");
    assert_eq!(
        config.services[1].upstream,
        Some("127.0.0.1:8080".parse().unwrap())
    );
    assert_eq!(config.services[2].url.as_str(), "https://random.test/");
    assert_eq!(config.services[2].node_id, "self");
    assert_eq!(
        config.services[2].upstream,
        Some("127.0.0.1:8000".parse().unwrap())
    );
}

#[test]
fn test_service_upstream_config() {
    let toml_data = r#"
        [[services]]
        url = "https://app.example.com"
        node_id = "vm-01"

        [[services]]
        url = "https://local.example.com"
        node_id = "self"
        upstream = "127.0.0.1:3000"
    "#;

    let config = Config::from_toml_str(toml_data).expect("failed to parse config toml");
    assert_eq!(config.services.len(), 2);
    assert_eq!(config.services[0].upstream, None);
    assert_eq!(
        config.services[1].upstream,
        Some("127.0.0.1:3000".parse().unwrap())
    );
}

#[test]
fn test_load_auto_cli_override() {
    let config = Config::load_auto(Some(std::path::Path::new("bridge.toml")))
        .expect("failed to load bridge.toml via cli override");
    assert_eq!(config.proxy.mode, ProxyMode::Direct);
    assert_eq!(config.nodes.len(), 2);
}

#[test]
fn test_load_auto_fallback_default() {
    // When no matching path is found or given a nonexistent cli path, it returns an error for invalid explicit path
    let err = Config::load_auto(Some(std::path::Path::new("nonexistent.toml")));
    assert!(err.is_err());

    // When no cli override or env var is set and candidates don't exist (tested with None)
    let auto_config = Config::load_auto(None).expect("failed auto load");
    // Since bridge.toml exists in the current directory, it discovers bridge.toml
    assert_eq!(auto_config.proxy.mode, ProxyMode::Direct);
    assert_eq!(auto_config.nodes.len(), 2);
}

#[test]
fn test_from_yaml_str() {
    let yaml_data = r#"
enable_telemetry: true

sentry:
  dsn: "https://yamlPublicKey@o0.ingest.sentry.io/1"
  environment: "production"
  sample_rate: 0.8
  traces_sample_rate: 0.2

logger:
  level: "INFO"
  format: "json"
  target: "stdout"

proxy:
  mode: "Handoff"
  routing: "direct"
  listeners:
    - "https"
    - "http"
  redirect_http: true
  max_concurrency: 32

nodes:
  - id: "vm-yaml"
    direct_endpoint: "198.51.100.55:443"
    mesh_endpoint: "10.8.0.55:443"
    routing: "direct"

services:
  - url: "https://yaml.example.com"
    node_id: "vm-yaml"
    upstream: "127.0.0.1:9000"

udp_services:
  - node: "vm-yaml"
    port: 5353
    upstream: "10.8.0.55:53"
    session_timeout: 45
"#;

    let config = Config::from_yaml_str(yaml_data).expect("failed to parse yaml config");
    assert!(config.enable_telemetry);
    assert_eq!(config.sentry.environment.as_deref(), Some("production"));
    assert_eq!(config.sentry.sample_rate, 0.8);
    assert_eq!(config.logger.level, tracing::Level::INFO);
    assert_eq!(config.proxy.mode, ProxyMode::Handoff);
    assert_eq!(config.proxy.routing, registry::RoutingPreference::Direct);
    assert_eq!(config.proxy.listeners, vec![Scheme::Https, Scheme::Http]);
    assert!(config.proxy.redirect_http);
    assert_eq!(config.proxy.max_concurrency, 32);

    assert_eq!(config.nodes.len(), 1);
    assert_eq!(config.nodes[0].node_id, "vm-yaml");
    assert_eq!(
        config.nodes[0].direct_address,
        Some("198.51.100.55:443".parse().unwrap())
    );
    assert_eq!(
        config.nodes[0].mesh_address,
        Some("10.8.0.55:443".parse().unwrap())
    );
    assert_eq!(
        config.nodes[0].routing,
        Some(registry::RoutingPreference::Direct)
    );
    assert_eq!(
        config.nodes[0].target_address(),
        "198.51.100.55:443".parse().unwrap()
    );

    assert_eq!(config.services.len(), 1);
    assert_eq!(config.services[0].url.as_str(), "https://yaml.example.com/");
    assert_eq!(
        config.services[0].upstream,
        Some("127.0.0.1:9000".parse().unwrap())
    );

    assert_eq!(config.udp_services.len(), 1);
    assert_eq!(config.udp_services[0].node_id, "vm-yaml");
    assert_eq!(config.udp_services[0].listen_port, 5353);
    assert_eq!(
        config.udp_services[0].session_timeout,
        std::time::Duration::from_secs(45)
    );
}

#[test]
fn test_from_file_yaml_and_yml() {
    let temp_dir = std::env::temp_dir();
    let yaml_path = temp_dir.join(format!("bridge-test-{}.yaml", std::process::id()));
    let yml_path = temp_dir.join(format!("bridge-test-{}.yml", std::process::id()));

    let yaml_content = r#"
proxy:
  mode: "Handoff"
  routing: "mesh"
nodes:
  - id: "vm-1"
    endpoint: "10.8.0.1:443"
"#;

    std::fs::write(&yaml_path, yaml_content).unwrap();
    std::fs::write(&yml_path, yaml_content).unwrap();

    let cfg_yaml = Config::from_file(&yaml_path).expect("failed to load .yaml file");
    assert_eq!(cfg_yaml.proxy.mode, ProxyMode::Handoff);
    assert_eq!(cfg_yaml.proxy.routing, registry::RoutingPreference::Mesh);
    assert_eq!(cfg_yaml.nodes.len(), 1);

    let cfg_yml = Config::from_file(&yml_path).expect("failed to load .yml file");
    assert_eq!(cfg_yml.proxy.mode, ProxyMode::Handoff);
    assert_eq!(cfg_yml.nodes.len(), 1);

    let auto_yaml = Config::load_auto(Some(&yaml_path)).expect("failed to load via auto cli");
    assert_eq!(auto_yaml.nodes.len(), 1);

    let _ = std::fs::remove_file(yaml_path);
    let _ = std::fs::remove_file(yml_path);
}
