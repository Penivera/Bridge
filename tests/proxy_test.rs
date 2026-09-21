use proxy::core::enums::{ProxyMode, Scheme};
use proxy::{Proxy, ProxyConfig};
use registry::DomainRegistry;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

#[test]
fn test_proxy_decoupled_construction() {
    let proxy_config = Arc::new(
        toml::from_str::<ProxyConfig>(
            r#"
        mode = "Handoff"
        listeners = ["https"]
        http_addr = "127.0.0.1:80"
        https_addr = "127.0.0.1:443"
        "#,
        )
        .unwrap(),
    );

    let registry = Arc::new(DomainRegistry::new());
    let proxy = Proxy::new(proxy_config.clone(), registry.clone());

    assert_eq!(proxy.config().mode, ProxyMode::Handoff);
    assert_eq!(proxy.config().listeners, vec![Scheme::Https]);
    assert!(!proxy.config().redirect_http);
    assert_eq!(proxy.config().http_addr(), "127.0.0.1:80".parse().unwrap());
    assert_eq!(
        proxy.config().https_addr(),
        "127.0.0.1:443".parse().unwrap()
    );
    assert_eq!(proxy.registry().len(), 0);
}

#[test]
fn test_proxy_config_defaults() {
    let config = ProxyConfig::default();
    assert_eq!(config.mode, ProxyMode::Handoff);
    assert_eq!(config.listeners, vec![Scheme::Http]);
    assert!(!config.redirect_http);
    assert_eq!(config.http_addr(), "0.0.0.0:80".parse().unwrap());
    assert_eq!(config.https_addr(), "0.0.0.0:443".parse().unwrap());
}

async fn assert_listeners_accept(count: usize) {
    let proxy = Proxy::new(
        Arc::new(ProxyConfig::default()),
        Arc::new(DomainRegistry::new()),
    );
    let mut listeners = Vec::new();
    let mut addresses = Vec::new();
    for _ in 0..count {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        addresses.push(listener.local_addr().unwrap());
        listeners.push(listener);
    }
    let run = proxy.run_listeners(listeners);
    tokio::pin!(run);
    let clients = async {
        for addr in &addresses {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        }
    };
    timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = &mut run => panic!("listeners stopped: {result:?}"),
            () = clients => {}
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn test_one_listener_accepts() {
    assert_listeners_accept(1).await;
}

#[tokio::test]
async fn test_both_listeners_accept() {
    assert_listeners_accept(2).await;
}

#[tokio::test]
async fn test_run_without_listeners() {
    let config = toml::from_str::<ProxyConfig>("mode = \"Direct\"\nlisteners = []").unwrap();
    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));
    timeout(Duration::from_secs(2), proxy.run())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn test_run_propagates_bind_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = toml::from_str::<ProxyConfig>(&format!(
        "mode = \"Direct\"\nlisteners = [\"http\"]\nhttp_addr = \"{}\"",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));
    let error = timeout(Duration::from_secs(2), proxy.run())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
}
