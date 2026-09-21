use hyper::server::conn::http1::Builder as ServerBuilder;
use hyper_util::rt::TokioIo;
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

/// Runs the accept loop for the provided TCP listeners, serving incoming connections.
pub async fn run_listeners(listeners: Vec<TcpListener>, svc: Proxy) -> std::io::Result<()> {
    let mut tasks = JoinSet::new();
    for listener in listeners {
        let svc = svc.clone();
        tasks.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let svc = svc.clone();
                        tokio::spawn(async move {
                            let io = TokioIo::new(stream);
                            if let Err(err) = ServerBuilder::new()
                                .preserve_header_case(true)
                                .title_case_headers(true)
                                .serve_connection(io, svc)
                                .await
                            {
                                tracing::debug!("error serving connection: {err}");
                            }
                        });
                    }
                    Err(error) => return Err(error),
                }
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.map_err(std::io::Error::other)??;
    }
    Ok(())
}
