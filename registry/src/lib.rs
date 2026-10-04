use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;

use serde::{Deserialize, Serialize};



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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub upstream: Option<SocketAddr>,
    pub node: Node,
}

impl Route {
    pub fn new(upstream: Option<SocketAddr>, node: Node) -> Self {
        Self { upstream, node }
    }

    /// Resolves the target destination address:
    /// Returns explicit `upstream` override if set, otherwise the node's target address.
    pub fn target_addr(&self) -> SocketAddr {
        self.target_addr_with_fallback(RoutingPreference::Mesh)
    }

    pub fn target_addr_with_fallback(&self, fallback: RoutingPreference) -> SocketAddr {
        self.upstream
            .unwrap_or_else(|| self.node.target_address_with_fallback(fallback))
    }
}

/// Thread-safe, lock-free domain routing table.
///
/// Readers call [`DomainRegistry::lookup`] on the hot path with zero contention.
/// Writers call [`DomainRegistry::insert`] / [`DomainRegistry::remove`] which atomically
/// swap the entire inner map (copy-on-write). Writes are infrequent (config reload,
/// gossip updates) so the clone cost is acceptable.
pub struct DomainRegistry {
    routes: ArcSwap<HashMap<String, Route>>,
}

impl DomainRegistry {
    /// Creates an empty registry.
        pub fn new() -> Self {
        Self {
            routes: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    /// Creates a registry pre-populated with the given routes.
    pub fn with_routes(routes: HashMap<String, Route>) -> Self {
        Self {
            routes: ArcSwap::from_pointee(routes),
        }
    }

    /// Lock-free domain lookup. Returns `None` if the domain is not registered.
    pub fn lookup(&self, domain: &str) -> Option<Route> {
        self.routes.load().get(domain).cloned()
    }

    /// Insert or update a route. Atomically swaps the inner map.
    pub fn insert(&self, domain: String, entry: Route) {
        let mut map = HashMap::clone(&self.routes.load());
        map.insert(domain, entry);
        self.routes.store(Arc::new(map));
    }

    /// Remove a route. Returns the removed entry if it existed.
    pub fn remove(&self, domain: &str) -> Option<Route> {
        let mut map = HashMap::clone(&self.routes.load());
        let removed = map.remove(domain);
        self.routes.store(Arc::new(map));
        removed
    }

    /// Returns the number of registered routes.
    pub fn len(&self) -> usize {
        self.routes.load().len()
    }

    /// Returns true if the registry contains no routes.
    pub fn is_empty(&self) -> bool {
        self.routes.load().is_empty()
    }

    /// Returns a snapshot of all current routes.
    pub fn snapshot(&self) -> Arc<HashMap<String, Route>> {
        self.routes.load_full()
    }
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

