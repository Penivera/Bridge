use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;

use serde::{Deserialize, Serialize};

pub mod ring;
pub use ring::{
    hash_composite_key, hash_key, hash_vnode_token, HashRing, RingNode, DEFAULT_VNODES_PER_NODE,
};
/// Routing preference for target nodes: direct (public IP) or mesh (WireGuard overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RoutingPreference {
    #[default]
    #[serde(alias = "Mesh", alias = "MESH")]
    Mesh,
    #[serde(alias = "Direct", alias = "DIRECT")]
    Direct,
}

fn default_node_address() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 0))
}

/// A single route entry: maps a domain to a target node and its routing endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Logical node identifier (e.g. "vm-03").
    #[serde(alias = "id")]
    pub node_id: String,
    /// Default or fallback endpoint to forward traffic to (e.g. 10.8.0.3:443).
    #[serde(default = "default_node_address", alias = "endpoint")]
    pub address: SocketAddr,
    /// WireGuard / mesh overlay endpoint.
    #[serde(default, alias = "mesh_endpoint")]
    pub mesh_address: Option<SocketAddr>,
    /// Direct / public IP endpoint.
    #[serde(default, alias = "direct_endpoint", alias = "public_endpoint")]
    pub direct_address: Option<SocketAddr>,
    /// Preferred routing mode: "mesh" or "direct".
    #[serde(default)]
    pub routing: Option<RoutingPreference>,
    /// Whether to send PROXY protocol v2 header to this node (e.g. for Traefik/Coolify real IP recovery).
    #[serde(default, alias = "send_proxy_protocol", alias = "proxy_protocol_v2")]
    pub proxy_protocol: Option<bool>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            address: default_node_address(),
            mesh_address: None,
            direct_address: None,
            routing: None,
            proxy_protocol: None,
        }
    }
}

impl Node {
    pub fn new(node_id: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            node_id: node_id.into(),
            address,
            mesh_address: None,
            direct_address: None,
            routing: None,
            proxy_protocol: None,
        }
    }

    pub fn with_routing(mut self, routing: RoutingPreference) -> Self {
        self.routing = Some(routing);
        self
    }

    pub fn with_proxy_protocol(mut self, enabled: bool) -> Self {
        self.proxy_protocol = Some(enabled);
        self
    }

    /// Determines if PROXY protocol v2 should be sent to this node,
    /// falling back to the provided default preference if unconfigured on the node.
    pub fn should_send_proxy_protocol(&self, fallback: bool) -> bool {
        self.proxy_protocol.unwrap_or(fallback)
    }

    pub fn with_mesh_address(mut self, addr: SocketAddr) -> Self {
        self.mesh_address = Some(addr);
        self
    }

    pub fn with_direct_address(mut self, addr: SocketAddr) -> Self {
        self.direct_address = Some(addr);
        self
    }

    /// Resolves the target address based on the node's routing preference,
    /// falling back to the provided fallback preference if none is configured on the node.
    pub fn target_address_with_fallback(&self, fallback: RoutingPreference) -> SocketAddr {
        let pref = self.routing.unwrap_or(fallback);
        match pref {
            RoutingPreference::Direct => {
                self.direct_address
                    .or(self.mesh_address)
                    .unwrap_or(self.address)
            }
            RoutingPreference::Mesh => {
                self.mesh_address
                    .or(self.direct_address)
                    .unwrap_or(self.address)
            }
        }
    }

    /// Resolves the target address based on the node's routing preference (defaulting to Mesh).
    pub fn target_address(&self) -> SocketAddr {
        self.target_address_with_fallback(RoutingPreference::Mesh)
    }
}

impl RingNode for Node {
    fn ring_node_id(&self) -> &str {
        &self.node_id
    }
}

/// Monotonically increasing Lamport vector clock for conflict resolution in gossip sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteClock {
    pub origin_node: String,
    pub version: u64,
}

impl RouteClock {
    pub fn new(origin_node: impl Into<String>, version: u64) -> Self {
        Self {
            origin_node: origin_node.into(),
            version,
        }
    }
}

impl PartialOrd for RouteClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RouteClock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.version
            .cmp(&other.version)
            .then_with(|| self.origin_node.cmp(&other.origin_node))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub upstream: Option<SocketAddr>,
    pub node: Node,
    #[serde(default)]
    pub targets: Vec<Node>,
    #[serde(default)]
    pub ring: Option<HashRing<Node>>,
    #[serde(default)]
    pub clock: Option<RouteClock>,
    #[serde(default)]
    pub is_tombstone: bool,
}

impl Route {
    pub fn new(upstream: Option<SocketAddr>, node: Node) -> Self {
        Self {
            upstream,
            node: node.clone(),
            targets: vec![node],
            ring: None,
            clock: None,
            is_tombstone: false,
        }
    }

    /// Creates a route with multiple target nodes, automatically initializing a 360-vnode hash ring.
    pub fn with_targets(upstream: Option<SocketAddr>, targets: Vec<Node>) -> Self {
        let primary = targets.first().cloned().unwrap_or_default();
        let ring = if targets.len() > 1 {
            Some(HashRing::from_nodes(targets.clone()))
        } else {
            None
        };
        Self {
            upstream,
            node: primary,
            targets,
            ring,
            clock: None,
            is_tombstone: false,
        }
    }

    /// Adds a target node to this route. If multiple targets exist, updates the hash ring.
    pub fn add_target(&mut self, node: Node) {
        if !self.targets.iter().any(|n| n.node_id == node.node_id) {
            self.targets.push(node);
        }
        if self.targets.len() > 1 {
            self.ring = Some(HashRing::from_nodes(self.targets.clone()));
        } else {
            self.ring = None;
        }
    }

    /// Removes a target node from this route.
    pub fn remove_target(&mut self, node_id: &str) -> bool {
        let initial_len = self.targets.len();
        self.targets.retain(|n| n.node_id != node_id);
        let removed = self.targets.len() < initial_len;
        if removed {
            if let Some(first) = self.targets.first() {
                self.node = first.clone();
            }
            if self.targets.len() > 1 {
                self.ring = Some(HashRing::from_nodes(self.targets.clone()));
            } else {
                self.ring = None;
            }
        }
        removed
    }

    /// Selects the target node for an incoming client connection using consistent hashing (session affinity).
    ///
    /// If only a single target exists, returns that node immediately.
    /// If multiple targets exist, hashes `SHA256(source_ip + hostname)` to lookup the sticky node.
    pub fn select_node_for_client(&self, client_ip: std::net::IpAddr, host: &str) -> &Node {
        if let Some(ref ring) = self.ring {
            ring.get_for_client(client_ip, host).unwrap_or(&self.node)
        } else {
            self.targets.first().unwrap_or(&self.node)
        }
    }

    /// Resolves the destination socket address for a client using consistent hashing if multi-node.
    pub fn target_addr_for_client(
        &self,
        client_ip: std::net::IpAddr,
        host: &str,
        fallback: RoutingPreference,
    ) -> SocketAddr {
        self.upstream.unwrap_or_else(|| {
            self.select_node_for_client(client_ip, host)
                .target_address_with_fallback(fallback)
        })
    }

    pub fn with_clock(mut self, origin_node: impl Into<String>, version: u64) -> Self {
        self.clock = Some(RouteClock::new(origin_node, version));
        self
    }

    pub fn tombstone(mut self) -> Self {
        self.is_tombstone = true;
        self
    }

    /// Resolves the target destination address:
    /// Returns explicit `upstream` override if set, otherwise the primary node's target address.
    pub fn target_addr(&self) -> SocketAddr {
        self.target_addr_with_fallback(RoutingPreference::Mesh)
    }

    pub fn target_addr_with_fallback(&self, fallback: RoutingPreference) -> SocketAddr {
        self.upstream
            .unwrap_or_else(|| self.node.target_address_with_fallback(fallback))
    }
}

/// Per-route traffic metrics recorded by the proxy hot path.
///
/// Counters are atomic; latency samples are kept in a small bounded ring
/// (latest 512) so percentile snapshots stay cheap and memory-bounded.
#[derive(Debug)]
pub struct RouteMetrics {
    pub requests: std::sync::atomic::AtomicU64,
    pub errors: std::sync::atomic::AtomicU64,
    latencies_ms: std::sync::Mutex<std::collections::VecDeque<u64>>,
}

impl RouteMetrics {
    const MAX_LATENCY_SAMPLES: usize = 512;

    fn new() -> Self {
        Self {
            requests: std::sync::atomic::AtomicU64::new(0),
            errors: std::sync::atomic::AtomicU64::new(0),
            latencies_ms: std::sync::Mutex::new(std::collections::VecDeque::new()),
        }
    }
}

/// Point-in-time, serialisable view of a route's metrics.
#[derive(Debug, Clone, Serialize)]
pub struct RouteMetricsSnapshot {
    pub requests: u64,
    pub errors: u64,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
}

/// Thread-safe, lock-free domain routing table.
///
/// Readers call [`DomainRegistry::lookup`] on the hot path with zero contention.
/// Writers call [`DomainRegistry::insert`] / [`DomainRegistry::remove`] which atomically
/// swap the entire inner map (copy-on-write). Writes are infrequent (config reload,
/// gossip updates) so the clone cost is acceptable.
pub struct DomainRegistry {
    routes: ArcSwap<HashMap<String, Route>>,
    metrics: std::sync::Mutex<HashMap<String, Arc<RouteMetrics>>>,
}

impl DomainRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            routes: ArcSwap::from_pointee(HashMap::new()),
            metrics: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Creates a registry pre-populated with the given routes.
    pub fn with_routes(routes: HashMap<String, Route>) -> Self {
        Self {
            routes: ArcSwap::from_pointee(routes),
            metrics: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Records one proxied request for `host` (port suffix is stripped).
    pub fn record_request(&self, host: &str, latency_ms: u64, is_error: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        let host = host.split(':').next().unwrap_or(host);
        let entry = {
            let mut map = match self.metrics.lock() {
                Ok(m) => m,
                Err(poisoned) => poisoned.into_inner(),
            };
            map.entry(host.to_string())
                .or_insert_with(|| Arc::new(RouteMetrics::new()))
                .clone()
        };
        entry.requests.fetch_add(1, Relaxed);
        if is_error {
            entry.errors.fetch_add(1, Relaxed);
        }
        let mut lat = match entry.latencies_ms.lock() {
            Ok(l) => l,
            Err(poisoned) => poisoned.into_inner(),
        };
        if lat.len() >= RouteMetrics::MAX_LATENCY_SAMPLES {
            lat.pop_front();
        }
        lat.push_back(latency_ms);
    }

    /// Returns a snapshot of all recorded per-route metrics.
    pub fn metrics_snapshot(&self) -> HashMap<String, RouteMetricsSnapshot> {
        use std::sync::atomic::Ordering::Relaxed;
        let map = match self.metrics.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.iter()
            .map(|(host, m)| {
                let mut samples: Vec<u64> = match m.latencies_ms.lock() {
                    Ok(l) => l.iter().copied().collect(),
                    Err(poisoned) => poisoned.into_inner().iter().copied().collect(),
                };
                samples.sort_unstable();
                let percentile = |p: usize| {
                    if samples.is_empty() {
                        None
                    } else {
                        // Nearest-rank: rank = ceil(p/100 * n), index rank-1.
                        let rank = (samples.len() * p).div_ceil(100);
                        Some(samples[rank.saturating_sub(1).min(samples.len() - 1)])
                    }
                };
                (
                    host.clone(),
                    RouteMetricsSnapshot {
                        requests: m.requests.load(Relaxed),
                        errors: m.errors.load(Relaxed),
                        p50_ms: percentile(50),
                        p95_ms: percentile(95),
                    },
                )
            })
            .collect()
    }

    /// Lock-free domain lookup. Returns `None` if the domain is not registered or is a tombstone.
    pub fn lookup(&self, domain: &str) -> Option<Route> {
        match self.routes.load().get(domain) {
            Some(r) if !r.is_tombstone => Some(r.clone()),
            _ => None,
        }
    }

    /// Lookup including tombstoned entries (used by gossip synchronization).
    pub fn lookup_raw(&self, domain: &str) -> Option<Route> {
        self.routes.load().get(domain).cloned()
    }

    /// Insert or update a route unconditionally. Atomically swaps the inner map.
    pub fn insert(&self, domain: String, entry: Route) {
        let mut map = HashMap::clone(&self.routes.load());
        map.insert(domain, entry);
        self.routes.store(Arc::new(map));
    }

    /// Inserts or updates a route only if its vector clock is strictly greater than the existing entry.
    /// Returns `true` if the route was inserted/updated, `false` if the incoming route was stale.
    pub fn insert_versioned(&self, domain: String, entry: Route) -> bool {
        let mut map = HashMap::clone(&self.routes.load());
        if let Some(existing) = map.get(&domain) {
            match (&entry.clock, &existing.clock) {
                (Some(in_clock), Some(ex_clock)) => {
                    if in_clock <= ex_clock {
                        return false;
                    }
                }
                (None, Some(_)) => return false,
                _ => {}
            }
        }
        map.insert(domain, entry);
        self.routes.store(Arc::new(map));
        true
    }

    /// Remove a route. Returns the removed entry if it existed.
    pub fn remove(&self, domain: &str) -> Option<Route> {
        let mut map = HashMap::clone(&self.routes.load());
        let removed = map.remove(domain);
        self.routes.store(Arc::new(map));
        removed
    }

    /// Generates a lightweight digest mapping all registered domains to their vector clocks.
    pub fn digest(&self) -> HashMap<String, RouteClock> {
        let map = self.routes.load();
        let mut digest = HashMap::with_capacity(map.len());
        for (domain, route) in map.iter() {
            if let Some(clock) = &route.clock {
                digest.insert(domain.clone(), clock.clone());
            } else {
                digest.insert(
                    domain.clone(),
                    RouteClock::new(route.node.node_id.clone(), 1),
                );
            }
        }
        digest
    }

    /// Computes the diff between a remote peer's digest and our local state.
    /// Returns:
    /// - `needed_from_remote`: Domains where the remote peer is ahead or we are missing.
    /// - `push_to_remote`: Full Route entries from our local registry where we are ahead.
    pub fn compare_digest(
        &self,
        remote_digest: &HashMap<String, RouteClock>,
    ) -> (Vec<String>, HashMap<String, Route>) {
        let local_map = self.routes.load();
        let mut needed_from_remote = Vec::new();
        let mut push_to_remote = HashMap::new();

        // 1. Check all entries in remote digest
        for (domain, remote_clock) in remote_digest {
            match local_map.get(domain) {
                Some(local_route) => {
                    let local_clock = local_route.clock.clone().unwrap_or_else(|| {
                        RouteClock::new(&local_route.node.node_id, 1)
                    });
                    if remote_clock > &local_clock {
                        needed_from_remote.push(domain.clone());
                    } else if &local_clock > remote_clock {
                        push_to_remote.insert(domain.clone(), local_route.clone());
                    }
                }
                None => {
                    needed_from_remote.push(domain.clone());
                }
            }
        }

        // 2. Check for local entries missing entirely from remote digest
        for (domain, local_route) in local_map.iter() {
            if !remote_digest.contains_key(domain) {
                push_to_remote.insert(domain.clone(), local_route.clone());
            }
        }

        (needed_from_remote, push_to_remote)
    }

    /// Returns the number of registered active routes.
    pub fn len(&self) -> usize {
        self.routes.load().values().filter(|r| !r.is_tombstone).count()
    }

    /// Returns true if the registry contains no active routes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a snapshot of all active (non-tombstone) routes.
    pub fn snapshot(&self) -> Arc<HashMap<String, Route>> {
        let map = self.routes.load();
        let filtered: HashMap<String, Route> = map
            .iter()
            .filter(|(_, r)| !r.is_tombstone)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Arc::new(filtered)
    }
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

