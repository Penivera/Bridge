use smart_default::SmartDefault;

#[derive(Clone, SmartDefault)]
pub struct Config {
    #[default(false)]
    pub enable_telemetry: bool,
    #[default(tracing::Level::DEBUG)]
    pub log_level: tracing::Level,
    pub proxy: ProxyConfig,
}

#[derive(Clone, SmartDefault)]
pub struct ProxyConfig {
    pub mode: proxy::core::enums::ProxyMode,
    /// Which schemes to listen on. Can contain Http, Https, or both.
    pub listeners: Vec<proxy::core::enums::Scheme>,
    /// When true and both Http and Https listeners are active,
    /// the Http listener redirects all traffic to Https instead of serving it.
    #[default(false)]
    pub redirect_http: bool,
}