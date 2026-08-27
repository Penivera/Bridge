



/// Operating modes for the Bridge proxy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// Direct mode: Bridge terminates TLS and acts as an L7 reverse proxy.
    /// Handles everything from VM routing to internal service routing and TLS termination.
    Direct,

    /// Handoff mode (Default): Bridge acts as an SNI-based L4 transparent passthrough router.
    /// Inspects the TLS ClientHello to extract the SNI hostname and transparently forwards
    /// the TCP stream to the destination VM's Coolify proxy.
    #[default]
    Handoff,
}

/// Scheme for the Bridge proxy layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scheme {
    /// Listen for http traffic only port:80.
    Http,
    /// Listen for https traffic only port:443.
    #[default]
    Https,
}

impl Scheme {
    pub fn port(&self) -> u16 {
        match self{
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}