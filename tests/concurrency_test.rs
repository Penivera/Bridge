use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use proxy::concurrency::{
    ElasticWorkerPool, QueueDwellMonitor, DEFAULT_SCALE_UP_THRESHOLD, SLIDING_WINDOW_SIZE,
};
use proxy::core::config::auto_detect_max_concurrency;
use proxy::{Proxy, ProxyConfig};
use registry::DomainRegistry;

#[test]
fn test_queue_dwell_monitor_sliding_window() {
    let mut monitor = QueueDwellMonitor::new(SLIDING_WINDOW_SIZE);
    assert_eq!(monitor.len(), 0);
    assert!(monitor.is_empty());
    assert_eq!(monitor.average_dwell(), Duration::ZERO);

    // Record 5 samples of 2ms
    for _ in 0..5 {
        monitor.record(Duration::from_millis(2));
    }
    assert_eq!(monitor.len(), 5);
    assert_eq!(monitor.average_dwell(), Duration::from_millis(2));
    assert!(!monitor.should_scale_up(DEFAULT_SCALE_UP_THRESHOLD)); // 2ms <= 5ms

    // Fill to 20 samples of 10ms
    for _ in 0..20 {
        monitor.record(Duration::from_millis(10));
    }
    // Length capped at capacity 20
    assert_eq!(monitor.len(), 20);
    assert_eq!(monitor.average_dwell(), Duration::from_millis(10));
    assert!(monitor.should_scale_up(DEFAULT_SCALE_UP_THRESHOLD)); // 10ms > 5ms

    // Adding 20 samples of 1ms should evict all previous 10ms samples
    for _ in 0..20 {
        monitor.record(Duration::from_millis(1));
    }
    assert_eq!(monitor.len(), 20);
    assert_eq!(monitor.average_dwell(), Duration::from_millis(1));
    assert!(!monitor.should_scale_up(DEFAULT_SCALE_UP_THRESHOLD));
}

#[test]
fn test_auto_detect_max_concurrency() {
    let detected = auto_detect_max_concurrency();
    assert!(
        (4..=64).contains(&detected),
        "detected concurrency {detected} out of range 4..=64"
    );
}

#[test]
fn test_max_concurrency_config_parsing() {
    let toml_explicit = r#"
        mode = "Direct"
        max_concurrency = 32
    "#;
    let config: ProxyConfig = toml::from_str(toml_explicit).expect("parse explicit failed");
    assert_eq!(config.max_concurrency, 32);

    let toml_auto = r#"
        mode = "Direct"
        max_concurrency = "auto"
    "#;
    let config_auto: ProxyConfig = toml::from_str(toml_auto).expect("parse auto failed");
    assert_eq!(config_auto.max_concurrency, auto_detect_max_concurrency());

    let toml_default = r#"
        mode = "Direct"
    "#;
    let config_default: ProxyConfig = toml::from_str(toml_default).expect("parse default failed");
    assert_eq!(config_default.max_concurrency, 20);
}

#[tokio::test]
async fn test_elastic_worker_pool_baseline_worker() {
    let proxy = Proxy::new(
        Arc::new(ProxyConfig::default()),
        Arc::new(DomainRegistry::new()),
    );
    let pool = ElasticWorkerPool::new(proxy, 10);

    // Initial baseline active worker should be 1
    assert_eq!(pool.active_workers(), 1);
    assert_eq!(pool.max_concurrency(), 10);

    pool.shutdown();
}

#[tokio::test]
async fn test_elastic_worker_pool_scale_up_under_load() {
    let proxy = Proxy::new(
        Arc::new(ProxyConfig::default()),
        Arc::new(DomainRegistry::new()),
    );
    let max_concurrency = 4;
    let pool = Arc::new(ElasticWorkerPool::with_options(
        proxy.clone(),
        max_concurrency,
        Duration::from_millis(1), // Low threshold for fast scale-up
        Duration::from_millis(200),
    ));

    // Bind listener
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let pool_dispatch = pool.clone();
    let accept_task = tokio::spawn(async move {
        for _ in 0..max_concurrency {
            if let Ok((stream, client_addr)) = listener.accept().await {
                pool_dispatch.dispatch(stream, client_addr);
            }
        }
    });

    // Connect 4 concurrent clients without closing immediately
    let mut clients = Vec::new();
    for _ in 0..max_concurrency {
        let stream = TcpStream::connect(addr).await.unwrap();
        clients.push(stream);
    }

    accept_task.await.unwrap();

    // Allow workers to spawn and take work
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Worker pool should have scaled up to accommodate concurrent load
    let active = pool.active_workers();
    assert!(
        active > 1,
        "expected pool to scale up beyond 1, got {active}"
    );

    pool.shutdown();
}

#[tokio::test]
async fn test_elastic_worker_pool_reaping_scale_down() {
    let proxy = Proxy::new(
        Arc::new(ProxyConfig::default()),
        Arc::new(DomainRegistry::new()),
    );
    let max_concurrency = 4;
    let pool = Arc::new(ElasticWorkerPool::with_options(
        proxy.clone(),
        max_concurrency,
        Duration::from_millis(1),
        Duration::from_millis(100), // Fast 100ms idle cooldown for testing
    ));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let pool_dispatch = pool.clone();
    let accept_task = tokio::spawn(async move {
        for _ in 0..max_concurrency {
            if let Ok((stream, client_addr)) = listener.accept().await {
                pool_dispatch.dispatch(stream, client_addr);
            }
        }
    });

    // Send HTTP requests and close connections
    for _ in 0..max_concurrency {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf).await;
    }

    accept_task.await.unwrap();

    // Wait for the idle cooldown duration (100ms) + buffer
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Should have reaped back down to the baseline active worker (1)
    let active_after_cooldown = pool.active_workers();
    assert_eq!(
        active_after_cooldown, 1,
        "expected active workers to shrink back to baseline 1, got {active_after_cooldown}"
    );

    pool.shutdown();
}

#[tokio::test]
async fn test_run_listeners_with_elastic_concurrency() {
    let mut config = ProxyConfig::default();
    config.mode = proxy::core::enums::ProxyMode::Direct;
    config.max_concurrency = 8;

    let proxy = Proxy::new(Arc::new(config), Arc::new(DomainRegistry::new()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let proxy_task = tokio::spawn({
        let proxy = proxy.clone();
        async move {
            let _ = proxy.run_listeners(vec![listener]).await;
        }
    });

    // Send multiple requests sequentially and concurrently
    for _ in 0..5 {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        let n = timeout(Duration::from_secs(2), stream.read_to_end(&mut resp))
            .await
            .expect("timeout")
            .unwrap();
        assert!(n > 0);
        assert!(resp.starts_with(b"HTTP/1.1 200 OK"));
    }

    proxy_task.abort();
}
