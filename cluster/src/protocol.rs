use std::net::{IpAddr, SocketAddr};
use serde::{Deserialize, Serialize};

/// 4-byte magic identifier for Bridge datagrams: "BRDG".
pub const MAGIC_BYTES: &[u8; 4] = b"BRDG";

/// Cluster protocol wire version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Represents a peer node participating in the WireGuard mesh and cluster protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerNode {
    /// Logical node identifier (e.g., "vm-01").
    pub node_id: String,
    /// WireGuard Curve25519 public key (base64).
    pub public_key: String,
    /// Overlay mesh IP inside the WireGuard subnet (e.g., 10.8.0.3).
    pub mesh_ip: IpAddr,
    /// Publicly reachable WireGuard & UDP cluster endpoint (e.g., 65.21.100.3:51820).
    pub endpoint: SocketAddr,
    /// Configurable election priority (higher priority wins Bully election, default 0).
    pub priority: u32,
}

impl PeerNode {
    pub fn new(
        node_id: impl Into<String>,
        public_key: impl Into<String>,
        mesh_ip: IpAddr,
        endpoint: SocketAddr,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            public_key: public_key.into(),
            mesh_ip,
            endpoint,
            priority: 0,
        }
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Evaluates the node's election ranking tuple `(priority, node_id)`.
    pub fn rank(&self) -> (u32, &str) {
        (self.priority, &self.node_id)
    }
}

impl registry::RingNode for PeerNode {
    fn ring_node_id(&self) -> &str {
        &self.node_id
    }
}

impl foca::Identity for PeerNode {
    type Addr = SocketAddr;

    fn renew(&self) -> Option<Self> {
        None
    }

    fn addr(&self) -> Self::Addr {
        self.endpoint
    }

    fn win_addr_conflict(&self, adversary: &Self) -> bool {
        self.rank() >= adversary.rank()
    }
}

/// Messages exchanged over raw UDP between Bridge nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterMessage {
    /// A new node requests to join the cluster by presenting its identity to a seed.
    JoinRequest {
        node: PeerNode,
    },
    /// Seed responds with the current active fleet of known peer nodes.
    JoinResponse {
        peers: Vec<PeerNode>,
    },
    /// A node informs other peers about a new peer joining the mesh.
    PeerAnnounce {
        peer: PeerNode,
    },
    /// Direct liveness probe (SWIM Ping).
    Ping {
        seq: u64,
        from_node: String,
    },
    /// Direct liveness acknowledgment (SWIM Ack).
    Ack {
        seq: u64,
        from_node: String,
    },
    /// Indirect liveness probe request (SWIM PingReq).
    PingReq {
        seq: u64,
        target_node: String,
        target_endpoint: SocketAddr,
    },
    /// Foca SWIM protocol payload frame (heartbeats, failure detection, membership gossip).
    SwimPayload(Vec<u8>),
    /// Routing table state digest for push-pull synchronization.
    RouteDigest {
        from_node: String,
        digest: std::collections::HashMap<String, registry::RouteClock>,
    },
    /// Request to pull full Route entries for specific domains from a peer whose clock is ahead.
    RoutePull {
        from_node: String,
        domains: Vec<String>,
    },
    /// Push update containing full Route entries (sent in response to RoutePull or preemptively).
    RouteSync {
        from_node: String,
        routes: std::collections::HashMap<String, registry::Route>,
    },
    /// Bully election message: candidate announces election to higher-ranked peers.
    Election {
        from_node: String,
        term: u64,
        priority: u32,
    },
    /// Bully election acknowledgment: higher-ranked peer informs candidate it will take over election.
    ElectionOk {
        from_node: String,
        term: u64,
    },
    /// Coordinator declaration: winning node announces its leadership to the cluster.
    Coordinator {
        leader: PeerNode,
        term: u64,
    },
    /// Coordinator acknowledgment: peer acknowledges new leader, used for quorum verification.
    CoordinatorAck {
        from_node: String,
        term: u64,
    },
    /// Request sent by the cluster leader instructing a node to spawn a duplicated workload for a failed service.
    SpawnWorkload {
        from_node: String,
        domain: String,
        image: String,
        env: Vec<String>,
        container_port: u16,
        origin_node: String,
    },
    /// Request sent by the cluster leader instructing a node to stop a duplicated workload after failback.
    StopWorkload {
        from_node: String,
        domain: String,
    },
    /// Acknowledgment sent by a node that has successfully spawned a duplicated workload.
    WorkloadSpawned {
        from_node: String,
        domain: String,
        target_addr: SocketAddr,
    },
}

#[derive(Debug)]
pub enum ProtocolError {
    PacketTooSmall(usize),
    InvalidMagic,
    UnsupportedVersion(u8),
    Deserialization(bincode::Error),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PacketTooSmall(len) => {
                write!(f, "packet too small: expected at least 5 bytes, got {len}")
            }
            Self::InvalidMagic => write!(f, "invalid magic header bytes: expected BRDG"),
            Self::UnsupportedVersion(ver) => {
                write!(
                    f,
                    "unsupported protocol version: expected {PROTOCOL_VERSION}, got {ver}"
                )
            }
            Self::Deserialization(err) => write!(f, "binary deserialization error: {err}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<bincode::Error> for ProtocolError {
    fn from(err: bincode::Error) -> Self {
        Self::Deserialization(err)
    }
}

impl ClusterMessage {
    /// Serializes the cluster message into a compact binary datagram with header.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        let payload = bincode::serialize(self)?;
        let mut buf = Vec::with_capacity(5 + payload.len());
        buf.extend_from_slice(MAGIC_BYTES);
        buf.push(PROTOCOL_VERSION);
        buf.extend_from_slice(&payload);
        Ok(buf)
    }

    /// Deserializes a cluster message from a raw UDP datagram buffer.
    pub fn decode(buf: &[u8]) -> Result<Self, ProtocolError> {
        if buf.len() < 5 {
            return Err(ProtocolError::PacketTooSmall(buf.len()));
        }
        if &buf[0..4] != MAGIC_BYTES {
            return Err(ProtocolError::InvalidMagic);
        }
        if buf[4] != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(buf[4]));
        }
        let msg = bincode::deserialize(&buf[5..])?;
        Ok(msg)
    }
}
