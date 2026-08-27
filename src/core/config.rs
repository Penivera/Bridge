

#[derive(Clone,Copy)]
pub struct Config {
    pub proxy_mode: proxy::core::enums::ProxyMode,
    pub proxy_scheme: proxy::core::enums::Scheme,
    pub log_level: tracing::Level,
}