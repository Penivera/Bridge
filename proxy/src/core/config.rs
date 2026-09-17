use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

use crate::core::enums::{ProxyMode, Scheme};

pub fn deserialize_listeners<'de, D>(
    deserializer: D,
) -> Result<Vec<Scheme>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let listeners = Vec::<Scheme>::deserialize(deserializer)?;

    if listeners.is_empty() || listeners.len() > 2 {
        return Err(serde::de::Error::custom(
            "listeners must contain 1 or 2 schemes",
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
}

impl ProxyConfig {
    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    pub fn https_addr(&self) -> SocketAddr {
        self.https_addr
    }
}
