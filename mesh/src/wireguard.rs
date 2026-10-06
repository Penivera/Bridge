use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};

/// Represents a single WireGuard peer configuration on a node's interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardPeer {
    /// Curve25519 public key of the peer (base64 string).
    pub public_key: String,
    /// Public UDP endpoint where the peer is reachable.
    pub endpoint: Option<SocketAddr>,
    /// Allowed IP ranges routed through this peer's tunnel (e.g., ["10.8.0.3/32"]).
    pub allowed_ips: Vec<String>,
    /// Persistent keepalive interval in seconds (default: 25 for NAT traversal).
    pub persistent_keepalive: u16,
}

impl WireGuardPeer {
    pub fn new(
        public_key: impl Into<String>,
        endpoint: Option<SocketAddr>,
        allowed_ips: Vec<String>,
    ) -> Self {
        Self {
            public_key: public_key.into(),
            endpoint,
            allowed_ips,
            persistent_keepalive: 25,
        }
    }

    pub fn with_keepalive(mut self, secs: u16) -> Self {
        self.persistent_keepalive = secs;
        self
    }
}

/// Generates a valid RFC 7748 Curve25519 WireGuard keypair encoded in base64.
///
/// Returns `(private_key_b64, public_key_b64)`.
pub fn generate_wireguard_keypair() -> (String, String) {
    let secret = StaticSecret::random();
    let public = PublicKey::from(&secret);

    let priv_b64 = BASE64_STANDARD.encode(secret.to_bytes());
    let pub_b64 = BASE64_STANDARD.encode(public.as_bytes());

    (priv_b64, pub_b64)
}

/// Validates that a string is a valid 32-byte base64 WireGuard key.
pub fn parse_wireguard_key(key_b64: &str) -> Result<[u8; 32], String> {
    let bytes = BASE64_STANDARD
        .decode(key_b64.trim())
        .map_err(|e| format!("invalid base64 key: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!(
            "invalid key length: expected 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// In-memory and system manager for a WireGuard interface (`wg0`).
#[derive(Debug, Clone)]
pub struct WireGuardDevice {
    pub interface_name: String,
    pub private_key: String,
    pub public_key: String,
    pub listen_port: u16,
    pub mesh_ip: IpAddr,
    peers: HashMap<String, WireGuardPeer>,
}

impl WireGuardDevice {
    /// Creates a new WireGuard device instance.
    pub fn new(
        interface_name: impl Into<String>,
        private_key: impl Into<String>,
        public_key: impl Into<String>,
        listen_port: u16,
        mesh_ip: IpAddr,
    ) -> Self {
        Self {
            interface_name: interface_name.into(),
            private_key: private_key.into(),
            public_key: public_key.into(),
            listen_port,
            mesh_ip,
            peers: HashMap::new(),
        }
    }

    /// Adds or updates a peer in the device's configuration.
    pub fn add_peer(&mut self, peer: WireGuardPeer) {
        tracing::info!(
            interface = %self.interface_name,
            peer_pubkey = %peer.public_key,
            endpoint = ?peer.endpoint,
            allowed_ips = ?peer.allowed_ips,
            "adding WireGuard mesh peer"
        );
        self.peers.insert(peer.public_key.clone(), peer);
    }

    /// Removes a peer by its public key.
    pub fn remove_peer(&mut self, public_key: &str) -> Option<WireGuardPeer> {
        let removed = self.peers.remove(public_key);
        if removed.is_some() {
            tracing::info!(
                interface = %self.interface_name,
                peer_pubkey = %public_key,
                "removed WireGuard mesh peer"
            );
        }
        removed
    }

    /// Retrieves an active peer by public key.
    pub fn get_peer(&self, public_key: &str) -> Option<&WireGuardPeer> {
        self.peers.get(public_key)
    }

    /// Returns a list of all configured peers.
    pub fn list_peers(&self) -> Vec<WireGuardPeer> {
        self.peers.values().cloned().collect()
    }

    /// Total count of configured peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Synchronizes the in-memory WireGuard device configuration (address,
    /// port, and all peers) with the host kernel interface via `defguard_wireguard_rs`.
    ///
    /// If running in an unprivileged environment (e.g. non-root unit test), logs a warning and returns `Ok(())`.
    pub fn sync_to_kernel(&self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            use defguard_wireguard_rs::key::Key;
            use defguard_wireguard_rs::net::IpAddrMask;
            use defguard_wireguard_rs::peer::Peer;
            use defguard_wireguard_rs::{InterfaceConfiguration, WGApi, WireguardInterfaceApi};
            let mut wgapi: WGApi = match WGApi::new(self.interface_name.clone()) {
                Ok(api) => api,
                Err(err) => {
                    tracing::warn!(interface = %self.interface_name, %err, "kernel WireGuard interface unavailable (unprivileged environment)");
                    return Ok(());
                }
            };

            let mut kernel_peers = Vec::with_capacity(self.peers.len());
            for peer in self.peers.values() {
                let key: Key = peer
                    .public_key
                    .as_str()
                    .try_into()
                    .map_err(|_| format!("invalid WireGuard peer public key: {}", peer.public_key))?;
                let mut kernel_peer = Peer::new(key);
                if let Some(endpoint) = &peer.endpoint {
                    kernel_peer
                        .set_endpoint(&endpoint.to_string())
                        .map_err(|e| format!("invalid WireGuard peer endpoint: {e}"))?;
                }
                let allowed_ips: Result<Vec<IpAddrMask>, _> = peer
                    .allowed_ips
                    .iter()
                    .map(|ip| ip.parse::<IpAddrMask>())
                    .collect();
                kernel_peer.set_allowed_ips(
                    allowed_ips.map_err(|e| format!("invalid WireGuard allowed IP: {e}"))?,
                );
                kernel_peer.persistent_keepalive_interval = Some(peer.persistent_keepalive);
                kernel_peers.push(kernel_peer);
            }

            let addr_str = format!("{}/32", self.mesh_ip);
            let addr = addr_str.parse().map_err(|e| format!("invalid mesh IP: {e}"))?;

            let config = InterfaceConfiguration {
                name: self.interface_name.clone(),
                prvkey: self.private_key.clone(),
                addresses: vec![addr],
                port: self.listen_port,
                peers: kernel_peers,
                mtu: None,
                fwmark: None,
            };

            if let Err(err) = wgapi.configure_interface(&config) {
                // The interface may not exist yet (first boot): create it, then retry.
                if let Err(create_err) = wgapi.create_interface() {
                    tracing::warn!(interface = %self.interface_name, %create_err, "could not create kernel WireGuard interface");
                }
                if let Err(retry_err) = wgapi.configure_interface(&config) {
                    tracing::warn!(interface = %self.interface_name, %err, %retry_err, "could not configure kernel WireGuard interface (requires CAP_NET_ADMIN)");
                }
            }
        }
        Ok(())
    }
}
