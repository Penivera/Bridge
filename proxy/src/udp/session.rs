use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};

/// Maximum datagram payload size supported by the UDP session.
pub const MAX_UDP_DATAGRAM_SIZE: usize = 65535;

/// Thread-safe map of active client sessions keyed by client socket address.
pub type SessionTable = Arc<RwLock<HashMap<SocketAddr, mpsc::Sender<Bytes>>>>;

/// Spawns an ephemeral UDP session for a specific client address.
///
/// Binds an ephemeral outbound socket connected to `upstream`, forwards inbound datagrams
/// to upstream, and routes responses back to `client_addr` via `ingress_socket`.
/// Automatically terminates after `session_timeout` of inactivity in both directions.
pub async fn spawn_session(
    client_addr: SocketAddr,
    upstream: SocketAddr,
    session_timeout: Duration,
    ingress_socket: Arc<UdpSocket>,
    sessions: SessionTable,
) -> std::io::Result<mpsc::Sender<Bytes>> {
    let bind_addr: SocketAddr = match upstream {
        SocketAddr::V4(_) => "0.0.0.0:0".parse().unwrap(),
        SocketAddr::V6(_) => "[::]:0".parse().unwrap(),
    };

    let outbound = Arc::new(UdpSocket::bind(bind_addr).await?);
    outbound.connect(upstream).await?;

    let (tx, mut rx) = mpsc::channel::<Bytes>(128);

    let outbound_send = outbound.clone();
    let outbound_recv = outbound.clone();
    let ingress = ingress_socket.clone();
    let sessions_cleanup = sessions.clone();

    let tx_for_cleanup = tx.clone();

    tokio::spawn(async move {
        let mut resp_buf = vec![0u8; MAX_UDP_DATAGRAM_SIZE];
        loop {
            tokio::select! {
                item = rx.recv() => {
                    match item {
                        Some(payload) => {
                            if let Err(err) = outbound_send.send(&payload).await {
                                tracing::debug!(%client_addr, %upstream, "error forwarding to upstream: {err}");
                                break;
                            }
                        }
                        None => {
                            tracing::debug!(%client_addr, "ingress channel closed, terminating session");
                            break;
                        }
                    }
                }
                res = outbound_recv.recv(&mut resp_buf) => {
                    match res {
                        Ok(n) if n > 0 => {
                            if let Err(err) = ingress.send_to(&resp_buf[..n], client_addr).await {
                                tracing::debug!(%client_addr, "error sending response to client: {err}");
                                break;
                            }
                        }
                        Ok(_) => break,
                        Err(err) => {
                            tracing::debug!(%client_addr, %upstream, "upstream recv error: {err}");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(session_timeout) => {
                    tracing::debug!(%client_addr, "session idle timeout reached, closing");
                    break;
                }
            }
        }

        let mut lock = sessions_cleanup.write().await;
        if let Some(existing) = lock.get(&client_addr)
            && existing.same_channel(&tx_for_cleanup)
        {
            lock.remove(&client_addr);
        }
    });

    Ok(tx)
}
