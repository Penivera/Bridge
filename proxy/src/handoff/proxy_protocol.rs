use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

/// Fixed 12-byte signature prefix defined by the PROXY Protocol v2 specification.
/// Hex: \x0D \x0A \x0D \x0A \x00 \x0D \x0A \x51 \x55 \x49 \x54 \x0A ("\r\n\r\n\0\r\nQUIT\n")
pub const PROXY_V2_PREFIX: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];

/// PROXY Protocol v2 Command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyCommand {
    /// Non-proxied connection (LOCAL). Used for health-checks or direct connections.
    Local,
    /// Connection forwarded on behalf of another entity (PROXY).
    Proxy,
}

/// PROXY Protocol v2 Transport Protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportProtocol {
    /// Stream-based transport (TCP)
    Stream,
    /// Datagram-based transport (UDP)
    Datagram,
    /// Unspecified transport
    Unspec,
}

/// Parsed PROXY Protocol v2 Header
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyV2Header {
    pub version: u8,
    pub command: ProxyCommand,
    pub transport_protocol: TransportProtocol,
    pub src_addr: SocketAddr,
    pub dst_addr: SocketAddr,
}

/// Encodes a binary PROXY Protocol v2 header for a TCP connection.
///
/// Prepending this header before streaming application bytes (e.g. TLS ClientHello)
/// allows downstream proxies (Traefik, Coolify, Nginx, HAProxy) to recover the real
/// client source IP and port for access logging, rate limiting, and GeoIP.
pub fn encode_proxy_v2(src: SocketAddr, dst: SocketAddr) -> Vec<u8> {
    encode_proxy_v2_with_protocol(src, dst, TransportProtocol::Stream)
}

/// Encodes a binary PROXY Protocol v2 header with explicit transport protocol.
pub fn encode_proxy_v2_with_protocol(
    src: SocketAddr,
    dst: SocketAddr,
    proto: TransportProtocol,
) -> Vec<u8> {
    let proto_nibble = match proto {
        TransportProtocol::Stream => 0x01,
        TransportProtocol::Datagram => 0x02,
        TransportProtocol::Unspec => 0x00,
    };

    match (src, dst) {
        (SocketAddr::V4(s), SocketAddr::V4(d)) => {
            let mut header = Vec::with_capacity(28);
            header.extend_from_slice(&PROXY_V2_PREFIX);
            header.push(0x21); // Version 2 | Command PROXY (0x01)
            header.push(0x10 | proto_nibble); // AF_INET (0x10) | Protocol
            header.extend_from_slice(&12u16.to_be_bytes()); // Address length = 12
            header.extend_from_slice(&s.ip().octets());
            header.extend_from_slice(&d.ip().octets());
            header.extend_from_slice(&s.port().to_be_bytes());
            header.extend_from_slice(&d.port().to_be_bytes());
            header
        }
        (s, d) => {
            // Mixed or IPv6: convert both to IPv6
            let src_v6 = match s {
                SocketAddr::V6(v6) => v6,
                SocketAddr::V4(v4) => SocketAddrV6::new(v4.ip().to_ipv6_mapped(), v4.port(), 0, 0),
            };
            let dst_v6 = match d {
                SocketAddr::V6(v6) => v6,
                SocketAddr::V4(v4) => SocketAddrV6::new(v4.ip().to_ipv6_mapped(), v4.port(), 0, 0),
            };

            let mut header = Vec::with_capacity(52);
            header.extend_from_slice(&PROXY_V2_PREFIX);
            header.push(0x21); // Version 2 | Command PROXY (0x01)
            header.push(0x20 | proto_nibble); // AF_INET6 (0x20) | Protocol
            header.extend_from_slice(&36u16.to_be_bytes()); // Address length = 36
            header.extend_from_slice(&src_v6.ip().octets());
            header.extend_from_slice(&dst_v6.ip().octets());
            header.extend_from_slice(&src_v6.port().to_be_bytes());
            header.extend_from_slice(&dst_v6.port().to_be_bytes());
            header
        }
    }
}

/// Parses a PROXY Protocol v2 header from an incoming byte slice.
///
/// Returns `Ok(Some((header, total_bytes_consumed)))` if a complete valid header is parsed.
/// Returns `Ok(None)` if the buffer is too short to contain the complete header.
/// Returns `Err(message)` if the data is not a valid PROXY v2 header.
pub fn parse_proxy_v2(bytes: &[u8]) -> Result<Option<(ProxyV2Header, usize)>, String> {
    if bytes.len() < 16 {
        return Ok(None);
    }

    if bytes[0..12] != PROXY_V2_PREFIX {
        return Err("invalid proxy protocol v2 signature prefix".to_string());
    }

    let ver_cmd = bytes[12];
    let version = ver_cmd >> 4;
    if version != 2 {
        return Err(format!("unsupported proxy protocol version: {version}"));
    }

    let command = match ver_cmd & 0x0F {
        0x00 => ProxyCommand::Local,
        0x01 => ProxyCommand::Proxy,
        other => return Err(format!("unknown proxy protocol command: 0x{other:02X}")),
    };

    let fam_proto = bytes[13];
    let family = fam_proto >> 4;
    let proto = match fam_proto & 0x0F {
        0x01 => TransportProtocol::Stream,
        0x02 => TransportProtocol::Datagram,
        _ => TransportProtocol::Unspec,
    };

    let addr_len = u16::from_be_bytes([bytes[14], bytes[15]]) as usize;
    let total_len = 16 + addr_len;

    if bytes.len() < total_len {
        return Ok(None); // Need more bytes
    }

    match (command, family) {
        (ProxyCommand::Local, _) => {
            // LOCAL command has no required address payload, return default unspecified address
            let default_addr = SocketAddr::from(([0, 0, 0, 0], 0));
            Ok(Some((
                ProxyV2Header {
                    version,
                    command,
                    transport_protocol: proto,
                    src_addr: default_addr,
                    dst_addr: default_addr,
                },
                total_len,
            )))
        }
        (ProxyCommand::Proxy, 0x01) => {
            // AF_INET (IPv4) - 12 bytes
            if addr_len < 12 {
                return Err(format!("proxy v2 ipv4 address block too short: {addr_len}"));
            }
            let src_ip = Ipv4Addr::new(bytes[16], bytes[17], bytes[18], bytes[19]);
            let dst_ip = Ipv4Addr::new(bytes[20], bytes[21], bytes[22], bytes[23]);
            let src_port = u16::from_be_bytes([bytes[24], bytes[25]]);
            let dst_port = u16::from_be_bytes([bytes[26], bytes[27]]);

            let src_addr = SocketAddr::V4(SocketAddrV4::new(src_ip, src_port));
            let dst_addr = SocketAddr::V4(SocketAddrV4::new(dst_ip, dst_port));

            Ok(Some((
                ProxyV2Header {
                    version,
                    command,
                    transport_protocol: proto,
                    src_addr,
                    dst_addr,
                },
                total_len,
            )))
        }
        (ProxyCommand::Proxy, 0x02) => {
            // AF_INET6 (IPv6) - 36 bytes
            if addr_len < 36 {
                return Err(format!("proxy v2 ipv6 address block too short: {addr_len}"));
            }
            let mut src_octets = [0u8; 16];
            src_octets.copy_from_slice(&bytes[16..32]);
            let mut dst_octets = [0u8; 16];
            dst_octets.copy_from_slice(&bytes[32..48]);

            let src_port = u16::from_be_bytes([bytes[48], bytes[49]]);
            let dst_port = u16::from_be_bytes([bytes[50], bytes[51]]);

            let src_addr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(src_octets), src_port, 0, 0));
            let dst_addr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(dst_octets), dst_port, 0, 0));

            Ok(Some((
                ProxyV2Header {
                    version,
                    command,
                    transport_protocol: proto,
                    src_addr,
                    dst_addr,
                },
                total_len,
            )))
        }
        (ProxyCommand::Proxy, other) => {
            Err(format!("unsupported proxy protocol address family: 0x{other:02X}"))
        }
    }
}
