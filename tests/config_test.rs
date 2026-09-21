use bridge::core::config::{Config, ProxyConfig};
use proxy::core::enums::ProxyMode;

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
