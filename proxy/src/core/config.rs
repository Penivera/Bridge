use std::{net::SocketAddr, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

use registry::RoutingPreference;
use crate::core::enums::{ProxyMode, Scheme};

pub fn deserialize_listeners<'de, D>(deserializer: D) -> Result<Vec<Scheme>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let listeners = Vec::<Scheme>::deserialize(deserializer)?;

    if listeners.len() > 2 {
        return Err(serde::de::Error::custom(
            "listeners must contain at most 2 schemes",
        ));
    }

    Ok(listeners)
}

/// Transport and listener settings for the Bridge proxy layer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, SmartDefault)]
#[serde(default)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    /// Which schemes to listen on. Can contain Http, Https, or both.
    /// In Managed mode this field is ignored (Bridge does not bind listener ports).
    #[default(vec![Scheme::Http])]
    #[serde(deserialize_with = "deserialize_listeners")]
    pub listeners: Vec<Scheme>,
    /// When true and both Http and Https listeners are active,
    /// the Http listener redirects all traffic to Https instead of serving it.
    /// Ignored in Managed mode.
    pub redirect_http: bool,
    #[default(SocketAddr::from(([0, 0, 0, 0], 80)))]
    pub http_addr: SocketAddr,
    #[default(SocketAddr::from(([0, 0, 0, 0], 443)))]
    pub https_addr: SocketAddr,
    #[default(Duration::from_secs(60))]
    pub keep_alive_duration: Duration,
    #[default(Duration::from_secs(60))]
    pub tcp_user_timeout: Duration,
    pub happy_eyeballs_timeout: Option<Duration>,
    pub keep_alive_retries: Option<u32>,
    #[serde(default)]
    pub udp_services: Vec<UdpServiceConfig>,
    /// Maximum worker task concurrency ceiling (default: 20, or "auto")
    #[default(default_max_concurrency())]
    #[serde(
        default = "default_max_concurrency",
        deserialize_with = "deserialize_max_concurrency"
    )]
    pub max_concurrency: usize,
    /// Default target node routing preference: direct (public IP) or mesh (WireGuard overlay).
    #[default(RoutingPreference::Mesh)]
    #[serde(default)]
    pub routing: RoutingPreference,
    /// Managed mode settings (Traefik dynamic configuration provider).
    #[serde(default)]
    pub managed: ManagedConfig,
    /// Whether to send PROXY protocol v2 headers in Handoff mode by default.
    /// Can be overridden per-node in the registry or nodes configuration.
    #[default(false)]
    #[serde(default, alias = "send_proxy_protocol", alias = "proxy_protocol_v2")]
    pub proxy_protocol: bool,
}

/// Default maximum concurrency worker task ceiling (20).
pub fn default_max_concurrency() -> usize {
    20
}

/// Helper function to auto-detect optimal concurrency ceiling based on hardware:
/// N_max = min(64, max(4, N_logical_cpus * 2))
pub fn auto_detect_max_concurrency() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (cpus * 2).clamp(4, 64)
}

pub fn deserialize_max_concurrency<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ConcurrencyValue {
        Num(usize),
        Str(String),
    }

    match ConcurrencyValue::deserialize(deserializer)? {
        ConcurrencyValue::Num(n) => Ok(std::cmp::max(1, n)),
        ConcurrencyValue::Str(s) if s.eq_ignore_ascii_case("auto") => {
            Ok(auto_detect_max_concurrency())
        }
        ConcurrencyValue::Str(s) => s
            .parse::<usize>()
            .map(|n| std::cmp::max(1, n))
            .map_err(serde::de::Error::custom),
    }
}

fn default_node() -> String {
    "self".to_string()
}

fn default_udp_session_timeout() -> Duration {
    Duration::from_secs(30)
}

fn deserialize_duration_or_secs<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DurationFormat {
        Secs(u64),
        Std(Duration),
    }

    match DurationFormat::deserialize(deserializer)? {
        DurationFormat::Secs(s) => Ok(Duration::from_secs(s)),
        DurationFormat::Std(d) => Ok(d),
    }
}

/// Configuration for a generic L4 UDP proxy service.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "UdpServiceConfigRaw")]
pub struct UdpServiceConfig {
    pub node_id: String,
    pub listen_port: u16,
    pub listen_addr: SocketAddr,
    pub upstream: SocketAddr,
    #[serde(
        default = "default_udp_session_timeout",
        deserialize_with = "deserialize_duration_or_secs"
    )]
    pub session_timeout: Duration,
}

#[derive(Deserialize)]
struct UdpServiceConfigRaw {
    #[serde(default = "default_node", alias = "node")]
    node_id: String,
    #[serde(default, alias = "port", alias = "node_port")]
    listen_port: Option<u16>,
    #[serde(default)]
    listen_addr: Option<SocketAddr>,
    #[serde(default)]
    upstream: Option<SocketAddr>,
    #[serde(
        default = "default_udp_session_timeout",
        deserialize_with = "deserialize_duration_or_secs"
    )]
    session_timeout: Duration,
}

impl TryFrom<UdpServiceConfigRaw> for UdpServiceConfig {
    type Error = String;

    fn try_from(raw: UdpServiceConfigRaw) -> Result<Self, Self::Error> {
        let (listen_addr, listen_port) = match (raw.listen_addr, raw.listen_port) {
            (Some(addr), Some(port)) => (addr, port),
            (Some(addr), None) => {
                let port = addr.port();
                (addr, port)
            }
            (None, Some(port)) => (SocketAddr::from(([0, 0, 0, 0], port)), port),
            (None, None) => {
                return Err(
                    "udp_service requires either `listen_port` (or `port`) or `listen_addr`"
                        .to_string(),
                );
            }
        };

        let upstream = raw
            .upstream
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], listen_port)));

        Ok(Self {
            node_id: raw.node_id,
            listen_port,
            listen_addr,
            upstream,
            session_timeout: raw.session_timeout,
        })
    }
}

impl UdpServiceConfig {
    pub fn new(listen_addr: SocketAddr, upstream: SocketAddr) -> Self {
        Self {
            node_id: default_node(),
            listen_port: listen_addr.port(),
            listen_addr,
            upstream,
            session_timeout: default_udp_session_timeout(),
        }
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = node_id.into();
        self
    }

    pub fn with_node(self, node: impl Into<String>) -> Self {
        self.with_node_id(node)
    }

    pub fn with_upstream(mut self, upstream: SocketAddr) -> Self {
        self.upstream = upstream;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.session_timeout = timeout;
        self
    }
}

impl ProxyConfig {
    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    pub fn https_addr(&self) -> SocketAddr {
        self.https_addr
    }

    pub fn keep_alive_duration(&self) -> Duration {
        self.keep_alive_duration
    }

    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }
}

/// Discovers the optimal Traefik dynamic configuration file path:
/// 1. `/data/coolify/proxy/dynamic/bridge.yaml` (Coolify host proxy directory)
/// 2. `/data/coolify/source/dynamic/bridge.yaml` (Coolify source dynamic directory)
/// 3. Falls back to `/etc/traefik/dynamic/bridge.yaml` (Standard Traefik path)
pub fn detect_traefik_dynamic_path() -> PathBuf {
    detect_traefik_dynamic_path_from(&[
        std::path::Path::new("/data/coolify/proxy/dynamic"),
        std::path::Path::new("/data/coolify/source/dynamic"),
    ])
}

/// Helper function to detect dynamic configuration path from a given slice of candidate directories.
pub fn detect_traefik_dynamic_path_from(candidates: &[&std::path::Path]) -> PathBuf {
    for candidate in candidates {
        if candidate.exists() {
            return candidate.join("bridge.yaml");
        }
    }
    PathBuf::from("/etc/traefik/dynamic/bridge.yaml")
}

fn default_dynamic_config_path() -> PathBuf {
    detect_traefik_dynamic_path()
}

fn default_entrypoints() -> Vec<String> {
    vec!["websecure".to_string()]
}

fn default_cert_resolver() -> Option<String> {
    Some("letsencrypt".to_string())
}

fn default_target_scheme() -> String {
    "http".to_string()
}

fn default_sync_interval() -> Duration {
    Duration::from_secs(5)
}

/// Configuration for Mode 3: Managed Mode (Coolify / Traefik dynamic provider).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, SmartDefault)]
#[serde(default)]
pub struct ManagedConfig {
    /// Path to Traefik dynamic configuration file.
    #[default(default_dynamic_config_path())]
    pub dynamic_config_path: PathBuf,

    /// Identifier for the local node (e.g. "self" or "vm-01").
    /// Routes targeting this node ID are skipped from Traefik dynamic config
    /// to avoid colliding with Coolify's local Docker container discovery.
    #[default(default_node())]
    pub local_node_id: String,

    /// Traefik entrypoints to bind routers to (e.g. `["websecure"]`).
    #[default(default_entrypoints())]
    pub entrypoints: Vec<String>,

    /// Whether to generate TLS router configuration.
    #[default(true)]
    pub tls_enabled: bool,

    /// TLS certificate resolver name in Traefik (e.g. "letsencrypt").
    /// If `None` and `tls_enabled` is true, empty TLS `{}` is configured (using Traefik default cert).
    #[default(default_cert_resolver())]
    pub cert_resolver: Option<String>,

    /// Target scheme for cross-node upstream servers ("http" or "https").
    /// Default: "http". (Port 443 targets always use "https").
    #[default(default_target_scheme())]
    pub default_target_scheme: String,

    /// Interval for periodic sync of domain registry changes to Traefik dynamic config file.
    #[default(default_sync_interval())]
    #[serde(
        default = "default_sync_interval",
        deserialize_with = "deserialize_duration_or_secs"
    )]
    pub sync_interval: Duration,
}

impl ManagedConfig {
    pub fn new(dynamic_config_path: impl Into<PathBuf>) -> Self {
        Self {
            dynamic_config_path: dynamic_config_path.into(),
            ..Default::default()
        }
    }

    pub fn with_local_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.local_node_id = node_id.into();
        self
    }

    pub fn with_entrypoints(mut self, entrypoints: Vec<String>) -> Self {
        self.entrypoints = entrypoints;
        self
    }

    pub fn with_tls_enabled(mut self, tls_enabled: bool) -> Self {
        self.tls_enabled = tls_enabled;
        self
    }

    pub fn with_cert_resolver(mut self, cert_resolver: Option<String>) -> Self {
        self.cert_resolver = cert_resolver;
        self
    }

    pub fn with_default_target_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.default_target_scheme = scheme.into();
        self
    }

    pub fn with_sync_interval(mut self, sync_interval: Duration) -> Self {
        self.sync_interval = sync_interval;
        self
    }
}

