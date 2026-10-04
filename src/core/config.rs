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
    #[serde(skip)]
    pub loaded_from: Option<std::path::PathBuf>,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Service {
    pub url: url::Url,
    pub node_id: String,
    #[serde(default)]
    pub upstream: Option<std::net::SocketAddr>,
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
}
