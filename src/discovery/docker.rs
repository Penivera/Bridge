use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use futures_util::StreamExt;
use registry::{DomainRegistry, Node, Route};
use tokio::sync::RwLock;

/// Parses domain names from a Traefik router rule string.
///
/// Handles patterns such as:
/// - `Host(`app.example.com`)`
/// - `Host(`app.example.com`, `admin.example.com`)`
/// - `Host("app.example.com")`
/// - `Host(`app.example.com`) || Host(`api.example.com`)`
/// - `Host(`app.example.com`) && PathPrefix(`/api`)`
pub fn parse_traefik_rule(rule: &str) -> Vec<String> {
    let mut domains = Vec::new();
    let lower_rule = rule.to_lowercase();
    let mut search_idx = 0;

    while let Some(pos) = lower_rule[search_idx..].find("host(") {
        let start_pos = search_idx + pos + 5; // after "host("
        if let Some(close_pos) = lower_rule[start_pos..].find(')') {
            let inner = &rule[start_pos..start_pos + close_pos];
            // Extract items separated by comma, delimited by backticks or quotes
            for token in inner.split(',') {
                let trimmed = token.trim();
                let stripped = trimmed
                    .trim_matches('`')
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim();
                if !stripped.is_empty()
                    && !stripped.contains(' ')
                    && !stripped.contains('(')
                    && !stripped.contains(')')
                    && !domains.contains(&stripped.to_string())
                {
                    domains.push(stripped.to_string());
                }
            }
            search_idx = start_pos + close_pos + 1;
        } else {
            break;
        }
    }

    domains
}

/// Parses Docker container labels into domain-to-route mappings.
///
/// Supported label formats:
/// - Traefik: `traefik.http.routers.<name>.rule = Host(...)`
/// - Port: `traefik.http.services.<name>.loadbalancer.server.port`
/// - Coolify: `coolify.domain = "app.example.com"`
/// - Generic: `bridge.domain = "app.example.com"`, `bridge.port = "3000"`
pub fn parse_docker_labels(
    labels: &HashMap<String, String>,
    default_node_id: &str,
) -> Vec<(String, Route)> {
    let mut detected_domains: Vec<String> = Vec::new();

    // 1. Check Traefik router rules
    for (k, v) in labels {
        if k.starts_with("traefik.http.routers.") && k.ends_with(".rule") {
            let hosts = parse_traefik_rule(v);
            for h in hosts {
                if !detected_domains.contains(&h) {
                    detected_domains.push(h);
                }
            }
        }
    }

    // 2. Check Coolify / Bridge direct domain labels
    for key in &["coolify.domain", "bridge.domain", "bridge.domains", "caddy_0"] {
        if let Some(val) = labels.get(*key) {
            for part in val.split([',', ' ', ';']) {
                let d = part.trim()
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_matches('/')
                    .trim();
                if !d.is_empty() && !detected_domains.contains(&d.to_string()) {
                    detected_domains.push(d.to_string());
                }
            }
        }
    }

    if detected_domains.is_empty() {
        return Vec::new();
    }

    // 3. Resolve target port
    let mut port: u16 = 80;

    // Check Traefik service port
    for (k, v) in labels {
        if k.starts_with("traefik.http.services.") && k.ends_with(".loadbalancer.server.port")
            && let Ok(p) = v.trim().parse::<u16>() {
                port = p;
                break;
            }
    }

    // Check explicit bridge/coolify port overrides
    for key in &["bridge.port", "coolify.port"] {
        if let Some(val) = labels.get(*key)
            && let Ok(p) = val.trim().parse::<u16>() {
                port = p;
                break;
            }
    }

    let target_addr = SocketAddr::from(([127, 0, 0, 1], port));
    let node = Node::new(default_node_id, target_addr);

    detected_domains
        .into_iter()
        .map(|domain| {
            let route = Route::new(Some(target_addr), node.clone());
            (domain, route)
        })
        .collect()
}

/// Discovers services by connecting to the local Docker daemon and watching container events.
pub struct DockerDiscovery {
    client: bollard::Docker,
    registry: Arc<DomainRegistry>,
    default_node_id: String,
    container_routes: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl DockerDiscovery {
    /// Creates a new `DockerDiscovery` with an existing `bollard::Docker` client.
    pub fn new(
        client: bollard::Docker,
        registry: Arc<DomainRegistry>,
        default_node_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            registry,
            default_node_id: default_node_id.into(),
            container_routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connects to Docker using default socket defaults (`/var/run/docker.sock` or `DOCKER_HOST`).
    pub fn connect_defaults(
        registry: Arc<DomainRegistry>,
        default_node_id: impl Into<String>,
    ) -> Result<Self, bollard::errors::Error> {
        let client = bollard::Docker::connect_with_socket_defaults()?;
        Ok(Self::new(client, registry, default_node_id))
    }

    /// Connects to Docker using an explicit Unix domain socket path.
    pub fn connect_socket(
        socket_path: &str,
        registry: Arc<DomainRegistry>,
        default_node_id: impl Into<String>,
    ) -> Result<Self, bollard::errors::Error> {
        let client = bollard::Docker::connect_with_unix(
            socket_path,
            120,
            bollard::API_DEFAULT_VERSION,
        )?;
        Ok(Self::new(client, registry, default_node_id))
    }

    /// Scans currently running containers on startup and populates the domain registry.
    pub async fn initial_scan(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let options = bollard::query_parameters::ListContainersOptions {
            all: false,
            ..Default::default()
        };

        let containers = self.client.list_containers(Some(options)).await?;
        let mut total_routes = 0;

        for container in containers {
            let Some(id) = container.id else { continue };
            let labels = container.labels.unwrap_or_default();
            let routes = parse_docker_labels(&labels, &self.default_node_id);

            if !routes.is_empty() {
                let mut domains_for_container = Vec::new();
                for (domain, route) in routes {
                    tracing::info!(
                        container_id = %id,
                        domain = %domain,
                        upstream = ?route.upstream,
                        "discovered docker service on initial scan"
                    );
                    self.registry.insert(domain.clone(), route);
                    domains_for_container.push(domain);
                    total_routes += 1;
                }
                let mut lock = self.container_routes.write().await;
                lock.insert(id, domains_for_container);
            }
        }

        Ok(total_routes)
    }

    /// Handles a container `start` event by inspecting the container and adding routes.
    pub async fn handle_container_start(
        &self,
        container_id: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let inspect = self.client.inspect_container(container_id, None).await?;
        let labels = inspect.config.and_then(|c| c.labels).unwrap_or_default();
        let routes = parse_docker_labels(&labels, &self.default_node_id);

        let mut added_domains = Vec::new();
        if !routes.is_empty() {
            for (domain, route) in routes {
                tracing::info!(
                    container_id = %container_id,
                    domain = %domain,
                    upstream = ?route.upstream,
                    "registering newly started container route"
                );
                self.registry.insert(domain.clone(), route);
                added_domains.push(domain);
            }
            let mut lock = self.container_routes.write().await;
            lock.insert(container_id.to_string(), added_domains.clone());
        }

        Ok(added_domains)
    }

    /// Handles a container `stop` or `die` event by removing its routes from the registry.
    pub async fn handle_container_stop(&self, container_id: &str) -> Vec<String> {
        let mut lock = self.container_routes.write().await;
        let Some(domains) = lock.remove(container_id) else {
            return Vec::new();
        };

        for domain in &domains {
            tracing::info!(
                container_id = %container_id,
                domain = %domain,
                "removing stopped container route"
            );
            self.registry.remove(domain);
        }

        domains
    }

    /// Runs the Docker discovery loop with a simple broadcast receiver.
    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = tokio::sync::broadcast::channel(1);
        tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            let _ = tx.send(proxy::ShutdownReason::Manual);
        });
        self.run_with_shutdown(rx, None).await
    }

    /// Runs the Docker discovery loop coordinating with Bridge's ShutdownCoordinator.
    pub async fn run_with_shutdown(
        self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<proxy::ShutdownReason>,
        ack_tx: Option<tokio::sync::mpsc::Sender<proxy::ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 1. Initial scan
        match self.initial_scan().await {
            Ok(count) => {
                tracing::info!(count, "docker initial scan completed successfully");
            }
            Err(err) => {
                tracing::warn!(%err, "failed initial docker scan (docker may be starting or unavailable)");
            }
        }

        // 2. Event stream
        let mut filters = HashMap::new();
        filters.insert("type".to_string(), vec!["container".to_string()]);
        filters.insert(
            "event".to_string(),
            vec![
                "start".to_string(),
                "die".to_string(),
                "stop".to_string(),
                "destroy".to_string(),
            ],
        );

        let options = bollard::query_parameters::EventsOptions {
            since: None,
            until: None,
            filters: Some(filters),
        };

        let mut stream = self.client.events(Some(options));

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::info!("docker discovery service received shutdown signal");
                    break;
                }
                event_opt = stream.next() => {
                    match event_opt {
                        Some(Ok(event)) => {
                            let action = event.action.as_deref().unwrap_or("");
                            let container_id = event.actor.as_ref().and_then(|a| a.id.as_deref()).unwrap_or("");
                            if container_id.is_empty() {
                                continue;
                            }

                            match action {
                                "start" => {
                                    if let Err(err) = self.handle_container_start(container_id).await {
                                        tracing::debug!(container_id, %err, "failed to inspect started container");
                                    }
                                }
                                "die" | "stop" | "destroy" => {
                                    self.handle_container_stop(container_id).await;
                                }
                                _ => {}
                            }
                        }
                        Some(Err(err)) => {
                            tracing::warn!(%err, "docker event stream error; backing off for 2s");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                        None => {
                            tracing::warn!("docker event stream closed unexpectedly");
                            break;
                        }
                    }
                }
            }
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(proxy::ShutdownAck {
                    subsystem: "docker_discovery".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}
