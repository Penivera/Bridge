use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use registry::DomainRegistry;
use tokio::net::TcpListener;
use hyper_util::{rt::TokioExecutor,client::legacy::{Client,connect::HttpConnector}};
use crate::core::config::ProxyConfig;
use crate::core::enums::ProxyMode;

mod handler;

/// Core ingress proxy engine for Bridge.
#[derive(Clone)]
pub struct Proxy {
    pub config: Arc<ProxyConfig>,
    pub registry: Arc<DomainRegistry>,
    pub client: Client<HttpConnector,BoxBody<Bytes,hyper::Error>>
}

impl Proxy {
    /// Creates a new `Proxy` instance with the given configuration and domain registry.
    pub fn new(config: Arc<ProxyConfig>, registry: Arc<DomainRegistry>) -> Self {
        let client = Client::builder(TokioExecutor::new()).build(HttpConnector::new());
        Self {
            config,
            registry,
            client
            
        }
    }

    /// Returns a reference to the proxy configuration.
    pub fn config(&self) -> &Arc<ProxyConfig> {
        &self.config
    }

    /// Returns a reference to the domain registry.
    pub fn registry(&self) -> &Arc<DomainRegistry> {
        &self.registry
    }

    /// Starts the proxy engine according to the configured `ProxyMode`.
    pub async fn run(&self) -> std::io::Result<()> {
        match self.config.mode {
            ProxyMode::Direct => {
                let listeners = crate::transport::bind_listeners(&self.config).await?;
                self.run_listeners(listeners).await
            }
            ProxyMode::Managed | ProxyMode::Handoff => Ok(()),
        }
    }

    /// Runs the accept loop on the provided listeners to serve incoming traffic.
    pub async fn run_listeners(&self, listeners: Vec<TcpListener>) -> std::io::Result<()> {
        crate::transport::run_listeners(listeners, self.clone()).await
    }
}

