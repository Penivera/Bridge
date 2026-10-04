use tokio::net::TcpListener;
use tokio::task::JoinSet;

use crate::Proxy;
use crate::core::config::ProxyConfig;
use crate::core::enums::Scheme;

/// Binds TCP listeners based on the provided proxy configuration.
pub async fn bind_listeners(config: &ProxyConfig) -> std::io::Result<Vec<TcpListener>> {
    let mut listeners = Vec::new();
    for scheme in &config.listeners {
        let addr = match scheme {
            Scheme::Http => config.http_addr(),
            Scheme::Https => config.https_addr(),
        };
        listeners.push(TcpListener::bind(addr).await?);
    }
    Ok(listeners)
}

/// Runs the accept loop for the provided TCP listeners, serving incoming connections
/// via the elastic work-stealing worker pool.
pub async fn run_listeners(listeners: Vec<TcpListener>, svc: Proxy) -> std::io::Result<()> {
    let pool = std::sync::Arc::new(crate::concurrency::ElasticWorkerPool::new(
        svc.clone(),
        svc.config.max_concurrency,
    ));

    let mut tasks = JoinSet::new();
    for listener in listeners {
        let pool = pool.clone();
        tasks.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, client_addr)) => {
                        pool.dispatch(stream, client_addr);
                    }
                    Err(error) => return Err(error),
                }
            }
        });
    }

    if let Some(result) = tasks.join_next().await {
        result.map_err(std::io::Error::other)??;
    }
    pool.shutdown();
    Ok(())
}

/// Runs the accept loop for TCP listeners with graceful shutdown coordination.
/// Listens for a broadcast shutdown signal, stops accepting connections, drains the worker pool,
/// and signals back via the mpsc acknowledgment channel.
pub async fn run_listeners_with_shutdown(
    listeners: Vec<TcpListener>,
    svc: Proxy,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<crate::shutdown::ShutdownReason>,
    ack_tx: tokio::sync::mpsc::Sender<crate::shutdown::ShutdownAck>,
) -> std::io::Result<()> {
    let pool = std::sync::Arc::new(crate::concurrency::ElasticWorkerPool::new(
        svc.clone(),
        svc.config.max_concurrency,
    ));

    let mut tasks = JoinSet::new();
    for listener in listeners {
        let pool = pool.clone();
        tasks.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, client_addr)) => {
                        pool.dispatch(stream, client_addr);
                    }
                    Err(error) => return Err(error),
                }
            }
        });
    }

    tokio::select! {
        reason = shutdown_rx.recv() => {
            tracing::info!(reason = ?reason, "tcp listeners received shutdown signal; stopping accept loop");
            tasks.abort_all();
            pool.shutdown();
            let peak = pool.peak_workers();
            let active = pool.active_workers();
            let _ = ack_tx.send(crate::shutdown::ShutdownAck {
                subsystem: "tcp_listeners".to_string(),
                details: Some(format!("peak_workers={}, remaining_active={}", peak, active)),
            }).await;
        }
        res = tasks.join_next() => {
            if let Some(result) = res {
                result.map_err(std::io::Error::other)??;
            }
            pool.shutdown();
        }
    }

    Ok(())
}
