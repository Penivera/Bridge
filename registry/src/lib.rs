use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;

use serde::{Deserialize, Serialize};



/// A single route entry: maps a domain to a target node and its WireGuard endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Logical node identifier (e.g. "vm-03").
    pub node_id: String,
    /// WireGuard endpoint to forward traffic to (e.g. 10.8.0.3:443).
    #[serde(alias = "endpoint")]
    pub address: SocketAddr,
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

