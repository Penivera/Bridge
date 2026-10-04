use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Long packet types defined in RFC 9000 Section 17.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicPacketType {
    Initial,
    ZeroRtt,
    Handshake,
    Retry,
}

/// Header summary extracted from a QUIC packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuicHeaderSummary {
    /// True if Long Header (RFC 9000), false if Short Header (1-RTT).
    pub is_long_header: bool,
    /// Packet type if long header.
    pub packet_type: Option<QuicPacketType>,
    /// QUIC version (4 bytes, e.g. 0x00000001 for QUIC v1) if long header.
    pub version: Option<u32>,
    /// Destination Connection ID (DCID).
    pub dcid: Vec<u8>,
    /// Source Connection ID (SCID) if long header.
    pub scid: Option<Vec<u8>>,
}

impl QuicHeaderSummary {
    /// Attempts to parse minimal QUIC header fields from raw datagram bytes.
    /// Returns `None` if the payload is too short or does not match QUIC framing.
    pub fn parse(payload: &[u8]) -> Option<Self> {
        if payload.is_empty() {
            return None;
        }

        let first_byte = payload[0];
        let is_long_header = (first_byte & 0x80) != 0;

        if is_long_header {
            // Long header must have at least 1 (flags) + 4 (version) + 1 (dcil) = 6 bytes
            if payload.len() < 6 {
                return None;
            }

            let version = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);

            let packet_type = match (first_byte >> 4) & 0x03 {
                0x00 => QuicPacketType::Initial,
                0x01 => QuicPacketType::ZeroRtt,
                0x02 => QuicPacketType::Handshake,
                0x03 => QuicPacketType::Retry,
                _ => return None,
            };

            let dcid_len = payload[5] as usize;
            if payload.len() < 6 + dcid_len + 1 {
                return None;
            }

            let dcid = payload[6..6 + dcid_len].to_vec();

            let scid_offset = 6 + dcid_len;
            let scid_len = payload[scid_offset] as usize;
            if payload.len() < scid_offset + 1 + scid_len {
                return None;
            }

            let scid = payload[scid_offset + 1..scid_offset + 1 + scid_len].to_vec();

            Some(Self {
                is_long_header: true,
                packet_type: Some(packet_type),
                version: Some(version),
                dcid,
                scid: Some(scid),
            })
        } else {
            // Short header (1-RTT). RFC 9000 Section 17.3.
            // DCID length is not self-describing in short headers; it is negotiated during handshake.
            // For routing inspection, we capture up to 20 bytes if available.
            Some(Self {
                is_long_header: false,
                packet_type: None,
                version: None,
                dcid: payload.get(1..std::cmp::min(payload.len(), 21)).unwrap_or_default().to_vec(),
                scid: None,
            })
        }
    }
}

/// Connection ID (CID) routing table for QUIC sessions in Bridge Handoff mode.
///
/// Maps Destination Connection IDs (DCIDs) to target node endpoints so subsequent
/// datagrams bypass SNI parsing and are transparently forwarded.
#[derive(Clone, Default)]
pub struct QuicRouter {
    routes: Arc<RwLock<HashMap<Vec<u8>, SocketAddr>>>,
}

impl QuicRouter {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Look up the target backend for a given Destination Connection ID.
    pub async fn lookup(&self, dcid: &[u8]) -> Option<SocketAddr> {
        let guard = self.routes.read().await;
        guard.get(dcid).copied()
    }

    /// Register a Destination Connection ID to a target backend.
    pub async fn register(&self, dcid: Vec<u8>, target: SocketAddr) {
        let mut guard = self.routes.write().await;
        guard.insert(dcid, target);
    }

    /// Remove a registered Connection ID.
    pub async fn remove(&self, dcid: &[u8]) -> Option<SocketAddr> {
        let mut guard = self.routes.write().await;
        guard.remove(dcid)
    }

    /// Number of active CID routes.
    pub async fn len(&self) -> usize {
        self.routes.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.routes.read().await.is_empty()
    }
}
