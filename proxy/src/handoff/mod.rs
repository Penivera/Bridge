use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use registry::{DomainRegistry, RoutingPreference};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub mod proxy_protocol;
pub mod sni;
pub use proxy_protocol::{
    encode_proxy_v2, encode_proxy_v2_with_protocol, parse_proxy_v2, ProxyCommand, ProxyV2Header,
    TransportProtocol, PROXY_V2_PREFIX,
};
pub use sni::{parse_http_host, parse_http_uri_path, parse_tls_sni};

/// Handles an incoming TCP connection in Mode 2 (Handoff Mode).
///
/// 1. Reads initial bytes (without TLS termination) to inspect the TLS ClientHello SNI or HTTP Host.
/// 2. If plaintext HTTP and `redirect_http` is true, sends an immediate HTTP 301 redirect to HTTPS.
/// 3. Resolves the destination VM endpoint from the domain registry according to the routing
///    preference (direct public IP or WireGuard mesh overlay).
/// 4. Establishes a transparent L4 connection to the target VM and forwards the initial bytes.
/// 5. If PROXY protocol v2 is enabled (per-node or globally), prepends the binary PROXY v2 header
///    so that downstream reverse proxies (Coolify/Traefik) recover the true client IP.
/// 6. Zero-copy bidirectionally splices the TCP streams (`tokio::io::copy_bidirectional`).
pub async fn handle_handoff_connection(
    mut client_stream: TcpStream,
    client_addr: SocketAddr,
    registry: Arc<DomainRegistry>,
    default_routing: RoutingPreference,
    redirect_http: bool,
    send_proxy_protocol: bool,
) -> std::io::Result<()> {
    let mut buf = vec![0u8; 4096];
    let n = client_stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    buf.truncate(n);

    // If it's a TLS record (0x16) and incomplete, attempt to read the rest of the record header
    if buf[0] == 0x16 && buf.len() >= 5 {
        let record_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
        let expected_total = std::cmp::min(5 + record_len, 16384);
        while buf.len() < expected_total {
            let mut temp = [0u8; 2048];
            match tokio::time::timeout(Duration::from_millis(500), client_stream.read(&mut temp)).await {
                Ok(Ok(read_bytes)) if read_bytes > 0 => {
                    buf.extend_from_slice(&temp[..read_bytes]);
                }
                _ => break,
            }
        }
    }

    let sni_opt = parse_tls_sni(&buf);
    let is_tls = sni_opt.is_some() || buf.first() == Some(&0x16);

    let domain = if let Some(sni) = sni_opt {
        sni
    } else if let Some(http_host) = parse_http_host(&buf) {
        if redirect_http {
            let uri = parse_http_uri_path(&buf);
            let redirect_resp = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: https://{}{}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                http_host, uri
            );
            let _ = client_stream.write_all(redirect_resp.as_bytes()).await;
            let _ = client_stream.flush().await;
            return Ok(());
        }
        http_host
    } else {
        tracing::debug!(client = %client_addr, "handoff: unable to extract SNI or Host header");
        return Ok(());
    };

    // Lookup domain in registry (handling optional trailing dot)
    let route = match registry.lookup(&domain).or_else(|| registry.lookup(domain.trim_end_matches('.'))) {
        Some(r) => r,
        None => {
            tracing::warn!(client = %client_addr, domain = %domain, "handoff: domain not found in registry");
            if !is_tls {
                let body = format!("No route found for host {domain}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = client_stream.write_all(resp.as_bytes()).await;
                let _ = client_stream.flush().await;
            }
            return Ok(());
        }
    };

    let target_addr = route.target_addr_with_fallback(default_routing);
    tracing::debug!(
        client = %client_addr,
        domain = %domain,
        node = %route.node.node_id,
        target = %target_addr,
        routing = ?route.node.routing.unwrap_or(default_routing),
        "handoff: forwarding connection"
    );

    let mut target_stream = match TcpStream::connect(target_addr).await {
        Ok(stream) => stream,
        Err(err) => {
            tracing::error!(
                client = %client_addr,
                target = %target_addr,
                domain = %domain,
                "handoff: failed to connect to target backend: {err}"
            );
            return Err(err);
        }
    };

    // If PROXY protocol v2 is enabled (either explicitly on the target node or via global default),
    // prepend the binary PROXY v2 header before streaming application bytes.
    if route.node.should_send_proxy_protocol(send_proxy_protocol) {
        let local_addr = client_stream.local_addr().unwrap_or(target_addr);
        let ppv2_header = encode_proxy_v2(client_addr, local_addr);
        target_stream.write_all(&ppv2_header).await?;
    }

    // Forward the initial buffered packet to the target backend
    target_stream.write_all(&buf).await?;

    // Splice client and target TCP streams bidirectionally
    let (from_client, from_target) = tokio::io::copy_bidirectional(&mut client_stream, &mut target_stream).await?;

    tracing::debug!(
        client = %client_addr,
        domain = %domain,
        target = %target_addr,
        bytes_client_to_target = from_client,
        bytes_target_to_client = from_target,
        "handoff: connection completed"
    );

    Ok(())
}
