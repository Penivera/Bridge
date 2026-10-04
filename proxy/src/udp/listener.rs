use std::collections::HashMap;
use std::sync::Arc;
use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

use crate::core::config::UdpServiceConfig;
use crate::udp::session::{spawn_session, SessionTable, MAX_UDP_DATAGRAM_SIZE};

/// An active UDP ingress listener that forwards incoming datagrams to an upstream address.
pub struct UdpListener {
    config: UdpServiceConfig,
}

impl UdpListener {
    /// Creates a new UDP listener for the given service configuration.
    pub fn new(config: UdpServiceConfig) -> Self {
        Self { config }
    }

    /// Binds the socket and begins proxying datagrams until shutdown or fatal I/O error.
    pub async fn run(&self) -> std::io::Result<()> {
        let socket = Arc::new(bind_udp_socket(self.config.listen_addr)?);
        run_udp_socket(socket, self.config.clone()).await
    }
}

/// Binds a UDP socket with SO_REUSEADDR and SO_REUSEPORT options.
pub fn bind_udp_socket(addr: std::net::SocketAddr) -> std::io::Result<UdpSocket> {
    let domain = match addr {
        std::net::SocketAddr::V4(_) => socket2::Domain::IPV4,
        std::net::SocketAddr::V6(_) => socket2::Domain::IPV6,
    };
    let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))?;
    let _ = socket.set_reuse_address(true);
    #[cfg(unix)]
    let _ = socket.set_reuse_port(true);
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    let std_sock: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_sock)
}

/// Runs datagram forwarding loop on an already bound UDP socket.
pub async fn run_udp_socket(socket: Arc<UdpSocket>, config: UdpServiceConfig) -> std::io::Result<()> {
    tracing::info!(
        listen_addr = %config.listen_addr,
        upstream = %config.upstream,
        session_timeout = ?config.session_timeout,
        "running UDP proxy listener"
    );

    let sessions: SessionTable = Arc::new(RwLock::new(HashMap::new()));
    let mut buf = vec![0u8; MAX_UDP_DATAGRAM_SIZE];

    loop {
        let (len, client_addr) = socket.recv_from(&mut buf).await?;
        if len == 0 {
            continue;
        }

        let data = Bytes::copy_from_slice(&buf[..len]);

        let maybe_tx = {
            let guard = sessions.read().await;
            guard.get(&client_addr).cloned()
        };

        match maybe_tx {
            Some(tx) => {
                if let Err(err) = tx.send(data).await {
                    tracing::debug!(%client_addr, "session channel closed, respawning: {err}");
                    match spawn_session(
                        client_addr,
                        config.upstream,
                        config.session_timeout,
                        socket.clone(),
                        sessions.clone(),
                    )
                    .await
                    {
                        Ok(new_tx) => {
                            let _ = new_tx.send(Bytes::copy_from_slice(&buf[..len])).await;
                            let mut write_guard = sessions.write().await;
                            write_guard.insert(client_addr, new_tx);
                        }
                        Err(e) => {
                            tracing::error!(%client_addr, %config.upstream, "failed to spawn UDP session: {e}");
                        }
                    }
                }
            }
            None => {
                match spawn_session(
                    client_addr,
                    config.upstream,
                    config.session_timeout,
                    socket.clone(),
                    sessions.clone(),
                )
                .await
                {
                    Ok(tx) => {
                        let _ = tx.send(data).await;
                        let mut write_guard = sessions.write().await;
                        write_guard.insert(client_addr, tx);
                    }
                    Err(e) => {
                        tracing::error!(%client_addr, %config.upstream, "failed to spawn UDP session: {e}");
                    }
                }
            }
        }
    }
}

/// Runs a single UDP proxy service.
pub async fn run_udp_service(config: UdpServiceConfig) -> std::io::Result<()> {
    let listener = UdpListener::new(config);
    listener.run().await
}

/// Concurrently runs multiple UDP proxy services.
pub async fn run_udp_services(services: Vec<UdpServiceConfig>) -> std::io::Result<()> {
    if services.is_empty() {
        return Ok(());
    }

    let mut tasks = JoinSet::new();
    for service in services {
        tasks.spawn(async move {
            run_udp_service(service).await
        });
    }

    while let Some(res) = tasks.join_next().await {
        res.map_err(std::io::Error::other)??;
    }

    Ok(())
}

/// Concurrently runs multiple UDP proxy services with graceful shutdown coordination.
pub async fn run_udp_services_with_shutdown(
    services: Vec<UdpServiceConfig>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<crate::shutdown::ShutdownReason>,
    ack_tx: tokio::sync::mpsc::Sender<crate::shutdown::ShutdownAck>,
) -> std::io::Result<()> {
    if services.is_empty() {
        let _ = ack_tx.send(crate::shutdown::ShutdownAck {
            subsystem: "udp_services".to_string(),
            details: Some("no services active".to_string()),
        }).await;
        return Ok(());
    }

    let mut tasks = JoinSet::new();
    let service_count = services.len();
    for service in services {
        tasks.spawn(async move {
            run_udp_service(service).await
        });
    }

    tokio::select! {
        reason = shutdown_rx.recv() => {
            tracing::info!(reason = ?reason, "udp services received shutdown signal; stopping listeners");
            tasks.abort_all();
            let _ = ack_tx.send(crate::shutdown::ShutdownAck {
                subsystem: "udp_services".to_string(),
                details: Some(format!("aborted {service_count} active udp listeners")),
            }).await;
        }
        res = tasks.join_next() => {
            if let Some(res) = res {
                res.map_err(std::io::Error::other)??;
            }
        }
    }

    Ok(())
}
