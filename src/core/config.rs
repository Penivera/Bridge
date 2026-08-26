

#[derive(Clone,Copy)]
pub struct Config {
    pub proxy_mode: proxy::core::enums::ProxyMode,
    pub tls: bool,
    pub log_level: tracing::Level,
}