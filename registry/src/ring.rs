use std::net::IpAddr;
use serde::{Deserialize, Serialize};

/// Default number of virtual nodes (tokens) per physical node on the hash ring.
///
/// 360 tokens provides optimal uniform distribution with low standard deviation
/// while keeping binary search lookups fast ($O(\log(360 \times N))$).
pub const DEFAULT_VNODES_PER_NODE: usize = 360;

/// Trait for items that can be placed on a consistent hash ring.
pub trait RingNode: Clone {
    /// Returns the unique identifier for this node (e.g. "vm-01", "node-a").
    fn ring_node_id(&self) -> &str;
}

impl RingNode for String {
    fn ring_node_id(&self) -> &str {
        self.as_str()
    }
}

impl RingNode for &str {
    fn ring_node_id(&self) -> &str {
        self
    }
}

/// A virtual node token placement on the ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualNode<T> {
    pub token: u64,
    pub node_id: String,
    pub node: T,
}

/// Hashes a node and its virtual node index into a 64-bit ring token using SHA-256.
pub fn hash_vnode_token(node_id: &str, vnode_idx: usize) -> u64 {
    let key = format!("{node_id}#{vnode_idx}");
    let digest = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
    let bytes: [u8; 8] = digest.as_ref()[..8]
        .try_into()
        .expect("SHA256 digest has at least 8 bytes");
    u64::from_be_bytes(bytes)
}

/// Hashes a generic string key into a 64-bit ring token using SHA-256.
pub fn hash_key(key: &str) -> u64 {
    let digest = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
    let bytes: [u8; 8] = digest.as_ref()[..8]
        .try_into()
        .expect("SHA256 digest has at least 8 bytes");
    u64::from_be_bytes(bytes)
}

/// Hashes a client IP and host into a 64-bit ring token (`SHA256(source_ip + hostname)`).
pub fn hash_composite_key(client_ip: IpAddr, host: &str) -> u64 {
    let key = format!("{client_ip}{host}");
    hash_key(&key)
}

/// A consistent hash ring managing token distribution across physical nodes.
///
/// Uses 360 virtual nodes per physical node by default and binary search lookup
/// to find the nearest clockwise node for any request key in $O(\log V)$ time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashRing<T> {
    vnodes_per_node: usize,
    ring: Vec<VirtualNode<T>>,
}

impl<T: RingNode> Default for HashRing<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: RingNode> HashRing<T> {
    /// Creates a new empty hash ring with the default 360 vnodes per node.
    pub fn new() -> Self {
        Self::with_vnodes(DEFAULT_VNODES_PER_NODE)
    }

    /// Creates a new empty hash ring with a specified number of vnodes per node.
    pub fn with_vnodes(vnodes_per_node: usize) -> Self {
        Self {
            vnodes_per_node: if vnodes_per_node == 0 {
                DEFAULT_VNODES_PER_NODE
            } else {
                vnodes_per_node
            },
            ring: Vec::new(),
        }
    }

    /// Creates a hash ring pre-populated with the given collection of nodes.
    pub fn from_nodes(nodes: impl IntoIterator<Item = T>) -> Self {
        let mut ring = Self::new();
        for node in nodes {
            ring.add_node(node);
        }
        ring
    }

    /// Number of virtual nodes configured per physical node.
    pub fn vnodes_per_node(&self) -> usize {
        self.vnodes_per_node
    }

    /// Returns the total number of virtual node tokens on the ring.
    pub fn vnode_count(&self) -> usize {
        self.ring.len()
    }

    /// Returns the count of distinct physical nodes on the ring.
    pub fn node_count(&self) -> usize {
        self.nodes().len()
    }

    /// Returns `true` if the ring has no nodes.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Adds a node to the ring, generating `vnodes_per_node` virtual tokens.
    /// If a node with the same `ring_node_id()` already exists, its previous tokens are replaced.
    pub fn add_node(&mut self, node: T) {
        let node_id = node.ring_node_id().to_string();
        // Remove existing vnodes for this node_id if present
        self.ring.retain(|v| v.node_id != node_id);

        for i in 0..self.vnodes_per_node {
            let token = hash_vnode_token(&node_id, i);
            self.ring.push(VirtualNode {
                token,
                node_id: node_id.clone(),
                node: node.clone(),
            });
        }

        // Sort ring tokens ascending; break ties using node_id
        self.ring
            .sort_by(|a, b| a.token.cmp(&b.token).then_with(|| a.node_id.cmp(&b.node_id)));
    }

    /// Removes a node from the ring by its identifier.
    /// Returns `true` if the node was present and removed, `false` otherwise.
    pub fn remove_node(&mut self, node_id: &str) -> bool {
        let initial_len = self.ring.len();
        self.ring.retain(|v| v.node_id != node_id);
        self.ring.len() < initial_len
    }

    /// Returns references to all distinct physical nodes registered on the ring.
    pub fn nodes(&self) -> Vec<&T> {
        let mut seen = std::collections::HashSet::new();
        let mut distinct = Vec::new();
        for v in &self.ring {
            if seen.insert(&v.node_id) {
                distinct.push(&v.node);
            }
        }
        distinct
    }

    /// Looks up the nearest clockwise node for a 64-bit token on the ring.
    pub fn get_by_token(&self, token: u64) -> Option<&T> {
        if self.ring.is_empty() {
            return None;
        }

        match self.ring.binary_search_by_key(&token, |v| v.token) {
            Ok(idx) => Some(&self.ring[idx].node),
            Err(idx) => {
                if idx < self.ring.len() {
                    Some(&self.ring[idx].node)
                } else {
                    // Clockwise wrap-around to the start of the ring
                    Some(&self.ring[0].node)
                }
            }
        }
    }

    /// Looks up the target node for a given string key (e.g. URI, domain, or arbitrary ID).
    pub fn get(&self, key: &str) -> Option<&T> {
        let token = hash_key(key);
        self.get_by_token(token)
    }

    /// Looks up the target node for a client IP and host name (`SHA256(source_ip + hostname)`).
    ///
    /// Provides session affinity: consecutive requests from the same client IP to the
    /// same service domain consistently route to the identical backend node.
    pub fn get_for_client(&self, client_ip: IpAddr, host: &str) -> Option<&T> {
        let token = hash_composite_key(client_ip, host);
        self.get_by_token(token)
    }
}
