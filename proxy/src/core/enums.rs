use serde::{Deserialize, Serialize};

/// Operating modes for the Bridge proxy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProxyMode {
    /// Direct mode: Bridge terminates TLS and acts as an L7 reverse proxy.
    /// Handles everything from VM routing to internal service routing and TLS termination.
    #[serde(alias = "direct", alias = "DIRECT")]
    Direct,

    /// Handoff mode (Default): Bridge acts as an SNI-based L4 transparent passthrough router.
    /// Inspects the TLS ClientHello to extract the SNI hostname and transparently forwards
    /// the TCP stream to the destination VM's Coolify proxy.
    #[default]
    #[serde(alias = "handoff", alias = "HANDOFF")]
    Handoff,

    /// Managed mode: Bridge does not proxy application traffic. Instead it acts as a
    /// control-plane component that dynamically configures the existing Coolify proxy
    /// (Traefik) on each VM via its provider API, injecting cross-node routing rules
    /// so that Traefik handles both local and remote service routing directly.
    #[serde(alias = "managed", alias = "MANAGED")]
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Listen for http traffic only port:80.
    #[serde(alias = "http", alias = "Http", alias = "HTTP")]
    Http,
    /// Listen for https traffic only port:443.
    #[serde(alias = "https", alias = "Https", alias = "HTTPS")]
    #[default]
    Https,
}

impl Scheme {
    pub fn port(&self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}
