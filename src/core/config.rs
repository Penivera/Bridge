use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

fn deserialize_log_level<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<tracing::Level, D::Error> {
    let value: String = serde::Deserialize::deserialize(d)?;
    value
        .parse::<tracing::Level>()
        .map_err(serde::de::Error::custom)
}

pub use proxy::{
    detect_traefik_dynamic_path, detect_traefik_dynamic_path_from, ManagedConfig, ProxyConfig,
    UdpServiceConfig,
};

#[derive(Clone, SmartDefault, Deserialize)]
#[serde(default)]
pub struct Config {
    #[default(false)]
    pub enable_telemetry: bool,
    pub sentry: SentryConfig,
    pub logger: LoggerConfig,
    pub proxy: ProxyConfig,
    pub nodes: Vec<registry::Node>,
    pub services: Vec<Service>,
    #[serde(default)]
    pub udp_services: Vec<UdpServiceConfig>,
    pub discovery: DiscoveryConfig,
    pub ipc: IpcConfig,
    pub dashboard: DashboardConfig,
    pub auth: AuthConfig,
    pub node: Option<NodeConfig>,
    #[serde(default)]
    pub seeds: Vec<SeedConfig>,
    #[serde(default)]
    pub handoff: HandoffConfig,
    #[serde(skip)]
    pub loaded_from: Option<std::path::PathBuf>,
}

fn default_dashboard_listen_addr() -> std::net::SocketAddr {
    "127.0.0.1:9090".parse().unwrap()
}

#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DashboardConfig {
    #[default(true)]
    pub enabled: bool,
    #[default(default_dashboard_listen_addr())]
    pub listen_addr: std::net::SocketAddr,
    /// Optional public domain routed to the dashboard through BRIDGE ingress.
    /// When set, only the elected cluster leader serves this route.
    pub public_domain: Option<String>,
}

/// Authentication configuration for the embedded dashboard.
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthConfig {
    #[default(false)]
    pub enabled: bool,
    pub users: Vec<AuthUser>,
}

/// A dashboard user with an Argon2 PHC password hash.
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthUser {
    pub username: String,
    pub password_hash: String,
}

#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DiscoveryConfig {
    #[default(true)]
    pub enabled: bool,
    #[default("/var/run/docker.sock".to_string())]
    pub docker_socket: String,
    #[default("self".to_string())]
    pub default_node_id: String,
}

#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct IpcConfig {
    #[default(true)]
    pub enabled: bool,
    #[default("/tmp/bridge.sock".to_string())]
    pub socket_path: String,
}

/// Helper resolving an environment variable reference if prefixed with "env:",
/// or returning the literal string otherwise.
pub fn resolve_env_str(val: &str) -> String {
    if let Some(var_name) = val.strip_prefix("env:") {
        std::env::var(var_name).unwrap_or_else(|_| val.to_string())
    } else {
        val.to_string()
    }
}

/// Ingress entrypoint handoff / failover configuration (Tier 1: none, Tier 2: dns, Tier 3a: tunnel, Tier 3b: floating_ip).
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct HandoffConfig {
    #[default(HandoffMode::None)]
    pub mode: HandoffMode,
    #[serde(default)]
    pub tunnel: Option<TunnelHandoffConfig>,
    #[serde(default)]
    pub dns: Option<DnsHandoffConfig>,
}

/// Ingress failover mode.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffMode {
    #[default]
    None,
    Tunnel,
    Dns,
    FloatingIp,
}

/// Cloudflare Tunnel handoff configuration (Tier 3a).
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct TunnelHandoffConfig {
    /// Cloudflare tunnel token (can use `env:CF_TUNNEL_TOKEN`).
    pub token: Option<String>,
    /// Cloudflare tunnel ID.
    pub tunnel_id: Option<String>,
    /// Path to tunnel credentials file (e.g. `/etc/cloudflared/cert.json`).
    pub credentials_file: Option<std::path::PathBuf>,
    /// Whether cloudflared stays running on all nodes (warm standby, faster failover)
    /// or only spawns on the elected leader (cold standby, lower resource use).
    #[default(false)]
    pub warm_standby: bool,
    /// Path to cloudflared executable (default: "cloudflared").
    #[default("cloudflared".to_string())]
    pub binary_path: String,
    /// Optional extra CLI arguments passed to cloudflared.
    #[default(vec![])]
    pub extra_args: Vec<String>,
}

/// DNS failover configuration (Tier 2).
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct DnsHandoffConfig {
    #[default("cloudflare".to_string())]
    pub provider: String,
    pub zone_id: Option<String>,
    pub record_name: Option<String>,
    pub record_id: Option<String>,
    pub api_token: Option<String>,
    #[default(60)]
    pub ttl: u32,
    #[default(false)]
    pub proxied: bool,
    pub target_ip: Option<std::net::IpAddr>,
    /// Optional API base URL override (default: "https://api.cloudflare.com/client/v4").
    #[default("https://api.cloudflare.com/client/v4".to_string())]
    pub api_base_url: String,
}

fn default_node_endpoint() -> std::net::SocketAddr {
    "127.0.0.1:51820".parse().unwrap()
}

fn default_node_mesh_ip() -> std::net::IpAddr {
    "10.8.0.1".parse().unwrap()
}

#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct NodeConfig {
    #[default("vm-01".to_string())]
    pub id: String,
    #[default(default_node_endpoint())]
    pub endpoint: std::net::SocketAddr,
    #[default(51820)]
    pub listen_port: u16,
    #[default(default_node_mesh_ip())]
    pub mesh_ip: std::net::IpAddr,
    pub private_key: Option<String>,
    pub public_key: Option<String>,
    #[default(0)]
    pub priority: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SeedConfig {
    #[serde(default)]
    pub id: Option<String>,
    pub endpoint: std::net::SocketAddr,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub mesh_ip: Option<std::net::IpAddr>,
}

#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SentryConfig {
    /// Sentry DSN (Data Source Name). Can also be provided via `SENTRY_DSN` env var.
    pub dsn: Option<String>,
    /// Environment tag (e.g. "production", "staging", "development")
    #[default(Some("production".to_string()))]
    pub environment: Option<String>,
    /// Error / event sample rate (0.0 to 1.0)
    #[default(1.0)]
    pub sample_rate: f32,
    /// Performance monitoring traces sample rate (0.0 to 1.0)
    #[default(0.0)]
    pub traces_sample_rate: f32,
    /// Sentry release identifier
    pub release: Option<String>,
    /// Print Sentry SDK debugging logs
    #[default(false)]
    pub debug: bool,
}

/// Strategy for recovering / failing back duplicated services when the original node recovers.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailbackMode {
    /// Keep the duplicate running on the failover node; do not kill in-flight sessions.
    #[default]
    NonPreemptive,
    /// When the original node is healthy past the cooldown, restore route and stop duplicate.
    Preemptive,
    /// Keep running until operator issues manual failback command.
    Manual,
}

/// Optional failover duplication configuration for a service.
#[derive(Clone, Debug, SmartDefault, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServiceReplicationConfig {
    #[default(false)]
    pub enabled: bool,
    pub image: Option<String>,
    #[default(vec![])]
    pub env: Vec<String>,
    pub container_port: Option<u16>,
    /// Placement strategy: "ring" (consistent hash successor), "leader", or explicit node_id
    #[default("ring".to_string())]
    pub placement: String,
    /// Failure recovery / failback strategy when original node recovers.
    #[default(FailbackMode::NonPreemptive)]
    pub failback_mode: FailbackMode,
    /// Cooldown seconds to wait before preemptive failback (default: 10s).
    #[default(10)]
    pub failback_cooldown_secs: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Service {
    pub url: url::Url,
    pub node_id: String,
    #[serde(default)]
    pub upstream: Option<std::net::SocketAddr>,
    #[serde(default)]
    pub replicate: Option<ServiceReplicationConfig>,
}

#[derive(SmartDefault, Deserialize, Clone)]
#[serde[default]]
pub struct LoggerConfig {
    #[serde(deserialize_with = "deserialize_log_level")]
    #[default(tracing::Level::DEBUG)]
    pub level: tracing::Level,
    #[default(LogFormat::Text)]
    pub format: LogFormat,
    #[default(LogTarget::Stderr)]
    pub target: LogTarget,
    #[default(true)]
    pub ansi: bool,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogTarget {
    #[default]
    Stderr,
    Stdout,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

impl Config {
    pub fn from_toml_str(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml_str)
    }

    pub fn from_file(
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        let file_data = std::fs::read_to_string(path_ref)?;
        let ext = path_ref
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut config = match ext.as_str() {
            "yaml" | "yml" => Self::from_yaml_str(&file_data)?,
            "toml" => Self::from_toml_str(&file_data)?,
            _ => {
                // If extension is unspecified or unrecognized, try TOML then YAML
                match Self::from_toml_str(&file_data) {
                    Ok(cfg) => cfg,
                    Err(toml_err) => Self::from_yaml_str(&file_data).map_err(|yaml_err| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("failed to parse as TOML ({toml_err}) or YAML ({yaml_err})"),
                        )
                    })?,
                }
            }
        };
        config.loaded_from = Some(path_ref.to_path_buf());
        Ok(config)
    }

    /// Discovers and loads the configuration file in precedence order:
    /// 1. Explicit CLI path override (if provided)
    /// 2. `BRIDGE_CONFIG` environment variable
    /// 3. Standard fallback candidate paths (`bridge.toml`, `bridge.yaml`, `bridge.yml`, `/etc/bridge/...`)
    /// 4. Falls back to default in-memory config if no configuration file is found
    pub fn load_auto(
        cli_override: Option<&std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(path) = cli_override {
            return Self::from_file(path);
        }

        if let Ok(env_path) = std::env::var("BRIDGE_CONFIG")
            && !env_path.trim().is_empty()
        {
            return Self::from_file(env_path);
        }

        let candidates = [
            std::path::PathBuf::from("bridge.toml"),
            std::path::PathBuf::from("bridge.yaml"),
            std::path::PathBuf::from("bridge.yml"),
            std::path::PathBuf::from("/etc/bridge/bridge.toml"),
            std::path::PathBuf::from("/etc/bridge/bridge.yaml"),
            std::path::PathBuf::from("/etc/bridge/bridge.yml"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Self::from_file(candidate);
            }
        }
        Ok(Self::default())
    }

    pub fn http_addr(&self) -> std::net::SocketAddr {
        self.proxy.http_addr()
    }

    pub fn https_addr(&self) -> std::net::SocketAddr {
        self.proxy.https_addr()
    }

    /// Discovers the active configuration file path if one exists on disk.
    pub fn discover_config_path(cli_override: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        if let Some(path) = cli_override {
            return Some(path.to_path_buf());
        }

        if let Ok(env_path) = std::env::var("BRIDGE_CONFIG")
            && !env_path.trim().is_empty()
        {
            let p = std::path::PathBuf::from(env_path);
            if p.exists() {
                return Some(p);
            }
        }

        let candidates = [
            std::path::PathBuf::from("bridge.toml"),
            std::path::PathBuf::from("bridge.yaml"),
            std::path::PathBuf::from("bridge.yml"),
            std::path::PathBuf::from("/etc/bridge/bridge.toml"),
            std::path::PathBuf::from("/etc/bridge/bridge.yaml"),
            std::path::PathBuf::from("/etc/bridge/bridge.yml"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return Some(candidate.clone());
            }
        }
        None
    }

    /// Automatically generates missing WireGuard Curve25519 keypairs and node defaults,
    /// persisting them back to the configuration file on disk.
    ///
    /// Returns `(changed, generated_public_key)`.
    pub fn auto_generate_missing_keys_and_save(
        path: &std::path::Path,
    ) -> Result<(bool, Option<String>), Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok((false, None));
        }
        let raw = std::fs::read_to_string(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut changed = false;
        let mut generated_pub_key = None;

        if ext == "yaml" || ext == "yml" {
            let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw)?;
            if let Some(node) = doc.get_mut("node").and_then(|v| v.as_mapping_mut()) {
                let has_priv = node
                    .get(&serde_yaml::Value::String("private_key".into()))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string());
                let has_pub = node
                    .get(&serde_yaml::Value::String("public_key".into()))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string());

                match (has_priv, has_pub) {
                    (None, _) => {
                        let (priv_k, pub_k) = mesh::generate_wireguard_keypair();
                        node.insert(
                            serde_yaml::Value::String("private_key".into()),
                            serde_yaml::Value::String(priv_k),
                        );
                        node.insert(
                            serde_yaml::Value::String("public_key".into()),
                            serde_yaml::Value::String(pub_k.clone()),
                        );
                        generated_pub_key = Some(pub_k);
                        changed = true;
                    }
                    (Some(priv_k), None) => {
                        if let Ok(pub_k) = mesh::derive_wireguard_public_key(&priv_k) {
                            node.insert(
                                serde_yaml::Value::String("public_key".into()),
                                serde_yaml::Value::String(pub_k.clone()),
                            );
                            generated_pub_key = Some(pub_k);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                let serialized = serde_yaml::to_string(&doc)?;
                Self::atomic_write_file(path, &serialized)?;
            }
        } else {
            let mut doc: toml::Value = toml::from_str(&raw)?;
            if let Some(node) = doc.get_mut("node").and_then(|v| v.as_table_mut()) {
                let has_priv = node
                    .get("private_key")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string());
                let has_pub = node
                    .get("public_key")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string());

                match (has_priv, has_pub) {
                    (None, _) => {
                        let (priv_k, pub_k) = mesh::generate_wireguard_keypair();
                        node.insert("private_key".into(), toml::Value::String(priv_k));
                        node.insert("public_key".into(), toml::Value::String(pub_k.clone()));
                        generated_pub_key = Some(pub_k);
                        changed = true;
                    }
                    (Some(priv_k), None) => {
                        if let Ok(pub_k) = mesh::derive_wireguard_public_key(&priv_k) {
                            node.insert("public_key".into(), toml::Value::String(pub_k.clone()));
                            generated_pub_key = Some(pub_k);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if changed {
                let serialized = toml::to_string_pretty(&doc)?;
                Self::atomic_write_file(path, &serialized)?;
            }
        }
        Ok((changed, generated_pub_key))
    }

    /// Automatically updates the configuration file with a newly discovered or updated peer,
    /// recording it as a seed node for persistent future bootstraps.
    pub fn persist_peer_to_config_file(
        path: &std::path::Path,
        peer: &cluster::PeerNode,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        if !path.exists() {
            return Ok(false);
        }
        let raw = std::fs::read_to_string(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "yaml" || ext == "yml" {
            let mut doc: serde_yaml::Value = serde_yaml::from_str(&raw)?;
            if let Some(local_id) = doc
                .get("node")
                .and_then(|n| n.get("id"))
                .and_then(|v| v.as_str())
            {
                if local_id == peer.node_id {
                    return Ok(false);
                }
            }
            let map = doc
                .as_mapping_mut()
                .ok_or_else(|| "config root is not a mapping".to_string())?;
            let seeds_key = serde_yaml::Value::String("seeds".into());
            let seeds = map
                .entry(seeds_key)
                .or_insert_with(|| serde_yaml::Value::Sequence(Vec::new()))
                .as_sequence_mut()
                .ok_or_else(|| "seeds is not a sequence".to_string())?;

            let mut matched = false;
            for s in seeds.iter_mut() {
                if let Some(s_map) = s.as_mapping_mut() {
                    let id_val = s_map
                        .get(&serde_yaml::Value::String("id".into()))
                        .and_then(|v| v.as_str());
                    let ep_val = s_map
                        .get(&serde_yaml::Value::String("endpoint".into()))
                        .and_then(|v| v.as_str());
                    if id_val == Some(&peer.node_id) || ep_val == Some(&peer.endpoint.to_string()) {
                        s_map.insert(
                            serde_yaml::Value::String("id".into()),
                            serde_yaml::Value::String(peer.node_id.clone()),
                        );
                        s_map.insert(
                            serde_yaml::Value::String("endpoint".into()),
                            serde_yaml::Value::String(peer.endpoint.to_string()),
                        );
                        s_map.insert(
                            serde_yaml::Value::String("public_key".into()),
                            serde_yaml::Value::String(peer.public_key.clone()),
                        );
                        s_map.insert(
                            serde_yaml::Value::String("mesh_ip".into()),
                            serde_yaml::Value::String(peer.mesh_ip.to_string()),
                        );
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                let mut new_seed = serde_yaml::Mapping::new();
                new_seed.insert(
                    serde_yaml::Value::String("id".into()),
                    serde_yaml::Value::String(peer.node_id.clone()),
                );
                new_seed.insert(
                    serde_yaml::Value::String("endpoint".into()),
                    serde_yaml::Value::String(peer.endpoint.to_string()),
                );
                new_seed.insert(
                    serde_yaml::Value::String("public_key".into()),
                    serde_yaml::Value::String(peer.public_key.clone()),
                );
                new_seed.insert(
                    serde_yaml::Value::String("mesh_ip".into()),
                    serde_yaml::Value::String(peer.mesh_ip.to_string()),
                );
                seeds.push(serde_yaml::Value::Mapping(new_seed));
            }
            let serialized = serde_yaml::to_string(&doc)?;
            Self::atomic_write_file(path, &serialized)?;
            Ok(true)
        } else {
            let mut doc: toml::Value = toml::from_str(&raw)?;
            if let Some(local_id) = doc
                .get("node")
                .and_then(|n| n.get("id"))
                .and_then(|v| v.as_str())
            {
                if local_id == peer.node_id {
                    return Ok(false);
                }
            }
            let root_tbl = doc
                .as_table_mut()
                .ok_or_else(|| "config root is not a table".to_string())?;
            let seeds = root_tbl
                .entry("seeds")
                .or_insert_with(|| toml::Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| "seeds is not an array".to_string())?;

            let mut matched = false;
            for s in seeds.iter_mut() {
                if let Some(s_tbl) = s.as_table_mut() {
                    let id_val = s_tbl.get("id").and_then(|v| v.as_str());
                    let ep_val = s_tbl.get("endpoint").and_then(|v| v.as_str());
                    if id_val == Some(&peer.node_id) || ep_val == Some(&peer.endpoint.to_string()) {
                        s_tbl.insert("id".into(), toml::Value::String(peer.node_id.clone()));
                        s_tbl.insert("endpoint".into(), toml::Value::String(peer.endpoint.to_string()));
                        s_tbl.insert("public_key".into(), toml::Value::String(peer.public_key.clone()));
                        s_tbl.insert("mesh_ip".into(), toml::Value::String(peer.mesh_ip.to_string()));
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                let mut new_seed = toml::map::Map::new();
                new_seed.insert("id".into(), toml::Value::String(peer.node_id.clone()));
                new_seed.insert("endpoint".into(), toml::Value::String(peer.endpoint.to_string()));
                new_seed.insert("public_key".into(), toml::Value::String(peer.public_key.clone()));
                new_seed.insert("mesh_ip".into(), toml::Value::String(peer.mesh_ip.to_string()));
                seeds.push(toml::Value::Table(new_seed));
            }
            let serialized = toml::to_string_pretty(&doc)?;
            Self::atomic_write_file(path, &serialized)?;
            Ok(true)
        }
    }

    fn atomic_write_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, path)
    }
}
