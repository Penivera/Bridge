pub mod error;
pub mod handlers;
pub mod stream;

use crate::dashboard::error::DashboardError;
use cluster::ClusterController;
use proxy::{ShutdownAck, ShutdownReason};
use registry::DomainRegistry;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, warn};

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use tower_http::timeout::TimeoutLayer;

use crate::auth::AuthManager;
use crate::failover::FailoverTrigger;

const ASSET_INDEX: &str = include_str!("assets/index.html");
const ASSET_LOGIN: &str = include_str!("assets/login.html");
const ASSET_404: &str = include_str!("assets/404.html");
const ASSET_FAVICON: &[u8] = include_bytes!("assets/favicon.png");
const ASSET_CSS: &str = include_str!("assets/style.css");
const ASSET_APP_JS: &str = include_str!("assets/app.js");
const ASSET_API_JS: &str = include_str!("assets/api.js");
const ASSET_VIEW_OVERVIEW: &str = include_str!("assets/views/overview.js");
const ASSET_VIEW_NODES: &str = include_str!("assets/views/nodes.js");
const ASSET_VIEW_ROUTES: &str = include_str!("assets/views/routes.js");
const ASSET_VIEW_MESH: &str = include_str!("assets/views/mesh.js");
const ASSET_VIEW_EVENTS: &str = include_str!("assets/views/events.js");
const ASSET_VIEW_CONFIG: &str = include_str!("assets/views/config.js");
const ASSET_VIEW_REQUESTS: &str = include_str!("assets/views/requests.js");
const ASSET_VIEW_BACKENDS: &str = include_str!("assets/views/backends.js");
const ASSET_VIEW_PROXY: &str = include_str!("assets/views/proxy.js");
const ASSET_VIEW_HANDOFFS: &str = include_str!("assets/views/handoffs.js");
const ASSET_VIEW_LOGS: &str = include_str!("assets/views/logs.js");
const ASSET_VIEW_HEALTH: &str = include_str!("assets/views/health.js");
const ASSET_VIEW_CLUSTER: &str = include_str!("assets/views/cluster.js");

const MAX_STORED_EVENTS: usize = 500;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardEvent {
    pub timestamp: String,
    pub event_type: String,
    pub node_id: Option<String>,
    pub domain: Option<String>,
    pub severity: String,
    pub message: String,
}

pub struct DashboardServer {
    pub listen_addr: SocketAddr,
    pub registry: Arc<DomainRegistry>,
    pub cluster: Arc<RwLock<Option<Arc<ClusterController>>>>,
    pub duplicator: Arc<RwLock<Option<Arc<dyn FailoverTrigger>>>>,
    pub start_time: Instant,
    pub event_tx: broadcast::Sender<String>,
    pub events: Arc<RwLock<VecDeque<DashboardEvent>>>,
    pub config_json: Arc<RwLock<Option<serde_json::Value>>>,
    pub config_path: Arc<RwLock<Option<std::path::PathBuf>>>,
    pub auth: Arc<AuthManager>,
    pub public_domain: Arc<RwLock<Option<String>>>,
}

impl DashboardServer {
    pub fn new(
        listen_addr: SocketAddr,
        registry: Arc<DomainRegistry>,
        cluster: Arc<RwLock<Option<Arc<ClusterController>>>>,
        duplicator: Arc<RwLock<Option<Arc<dyn FailoverTrigger>>>>,
        auth: Arc<AuthManager>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            listen_addr,
            registry,
            cluster,
            duplicator,
            start_time: Instant::now(),
            event_tx,
            events: Arc::new(RwLock::new(VecDeque::new())),
            config_json: Arc::new(RwLock::new(None)),
            config_path: Arc::new(RwLock::new(None)),
            auth,
            public_domain: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_public_domain(&self, domain: Option<String>) {
        let slot = self.public_domain.clone();
        tokio::spawn(async move {
            *slot.write().await = domain;
        });
    }

    pub fn set_config(&self, config: serde_json::Value, path: Option<std::path::PathBuf>) {
        let config_clone = self.config_json.clone();
        let path_clone = self.config_path.clone();
        tokio::spawn(async move {
            *config_clone.write().await = Some(config);
            *path_clone.write().await = path;
        });
    }

    pub async fn run_with_shutdown(
        self: Arc<Self>,
        mut shutdown_rx: broadcast::Receiver<ShutdownReason>,
        ack_tx: Option<mpsc::Sender<ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.spawn_membership_events();

        let app = Router::new()
            .route("/", get(serve_index))
            .route("/ui", get(serve_index))
            .route("/assets/{*path}", get(serve_asset))
            .route("/api/v1/status", get(handlers::handle_status))
            .route("/api/status", get(handlers::handle_status))
            .route("/api/v1/nodes", get(handlers::handle_nodes))
            .route("/api/v1/nodes/{id}", get(handlers::handle_node_detail))
            .route("/api/v1/routes", get(handlers::handle_routes))
            .route("/api/v1/routes/{hostname}", get(handlers::handle_route_detail))
            .route("/api/v1/mesh", get(handlers::handle_mesh))
            .route("/api/v1/metrics", get(handlers::handle_metrics))
            .route("/api/v1/events", get(handlers::handle_events))
            .route("/api/v1/handoff", get(handlers::handle_handoff))
            .route("/api/v1/config", get(handlers::handle_config))
            .route("/api/v1/config", axum::routing::put(handlers::handle_put_config))
            .route("/api/v1/logs", get(handlers::handle_logs))
            .route("/api/v1/stream", get(stream::ws_handler))
            .route("/api/v1/replicate", post(handlers::handle_replicate))
            .route("/api/replicate", post(handlers::handle_replicate))
            .route("/api/v1/failback", post(handlers::handle_failback))
            .route("/api/failback", post(handlers::handle_failback))
            .fallback(fallback_handler)
            .route_layer(axum::middleware::from_fn_with_state(
                self.clone(),
                require_auth,
            ))
            .route("/login", get(serve_login))
            .route("/favicon.png", get(serve_favicon))
            .route("/health", get(handlers::handle_health))
            .route("/healthz", get(handlers::handle_health))
            .route("/api/v1/login", post(handlers::handle_login))
            .route("/api/v1/logout", post(handlers::handle_logout))
            .route("/api/v1/auth", get(handlers::handle_auth))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                REQUEST_TIMEOUT,
            ))
            .with_state(self.clone());

        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Dashboard listening on {}", self.listen_addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.recv().await;
                info!("Dashboard shutting down");
            })
            .await?;

        if let Some(tx) = ack_tx {
            let _ = tx
                .send(ShutdownAck {
                    subsystem: "dashboard".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }

    fn spawn_membership_events(self: &Arc<Self>) {
        let server = self.clone();
        tokio::spawn(async move {
            // The cluster slot is populated after the dashboard starts
            // (daemon spawns the dashboard before binding the cluster), so
            // wait until a controller appears before subscribing.
            let cluster_ctl = loop {
                if let Some(ctrl) = server.cluster.read().await.as_ref() {
                    break ctrl.clone();
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            };

            let mut mem_rx = cluster_ctl.subscribe_membership();
            loop {
                match mem_rx.recv().await {
                    Ok(event) => {
                        let (event_type, severity, node) = match event {
                            cluster::MemberEvent::Up(node) => ("NODE_JOINED", "info", node),
                            cluster::MemberEvent::Down(node) => ("NODE_DEAD", "critical", node),
                        };

                        let db_event = DashboardEvent {
                            timestamp: crate::core::telemetry::now_rfc3339(),
                            event_type: event_type.to_string(),
                            node_id: Some(node.node_id.clone()),
                            domain: None,
                            severity: severity.to_string(),
                            message: format!("{event_type}: {}", node.node_id),
                        };

                        {
                            let mut events = server.events.write().await;
                            if events.len() >= MAX_STORED_EVENTS {
                                events.pop_front();
                            }
                            events.push_back(db_event.clone());
                        }
                        match serde_json::to_string(&db_event) {
                            Ok(json_str) => {
                                let _ = server.event_tx.send(json_str);
                            }
                            Err(e) => {
                                warn!("failed to serialise dashboard event: {e}");
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("membership event receiver lagged by {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}

async fn serve_index() -> Html<&'static str> {
    Html(ASSET_INDEX)
}

async fn serve_login() -> Html<&'static str> {
    Html(ASSET_LOGIN)
}

async fn serve_favicon() -> Response {
    ([(header::CONTENT_TYPE, "image/png")], ASSET_FAVICON).into_response()
}

pub const SESSION_COOKIE: &str = "bridge_session";

/// Extracts the session token from the request's Cookie header.
pub fn session_token_from_request<B>(req: &axum::http::Request<B>) -> Option<String> {
    session_token_from_headers(req.headers())
}

/// Extracts the session token from a header map's Cookie header.
pub fn session_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let mut parts = pair.trim().splitn(2, '=');
        let name = parts.next()?.trim();
        if name == SESSION_COOKIE {
            return parts.next().map(|v| v.trim().to_string());
        }
    }
    None
}

/// Authentication middleware: enforces session validity when auth is enabled.
async fn require_auth(
    axum::extract::State(server): axum::extract::State<Arc<DashboardServer>>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    if !server.auth.enabled() {
        return next.run(req).await;
    }

    let mut authenticated = false;
    if let Some(token) = session_token_from_request(&req) {
        authenticated = server.auth.validate(&token).await.is_some();
    }

    if authenticated {
        return next.run(req).await;
    }

    if req.uri().path().starts_with("/api/") {
        (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"code":"UNAUTHORIZED","error":"authentication required"}"#,
        )
            .into_response()
    } else {
        axum::response::Redirect::temporary("/login").into_response()
    }
}

async fn serve_asset(Path(path): Path<String>) -> Result<Response, DashboardError> {
    let (content, mime): (&[u8], &str) = match path.as_str() {
        "favicon.png" => (ASSET_FAVICON, "image/png"),
        "style.css" => (ASSET_CSS.as_bytes(), "text/css; charset=utf-8"),
        "app.js" => (ASSET_APP_JS.as_bytes(), "application/javascript; charset=utf-8"),
        "api.js" => (ASSET_API_JS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/overview.js" => (ASSET_VIEW_OVERVIEW.as_bytes(), "application/javascript; charset=utf-8"),
        "views/nodes.js" => (ASSET_VIEW_NODES.as_bytes(), "application/javascript; charset=utf-8"),
        "views/routes.js" => (ASSET_VIEW_ROUTES.as_bytes(), "application/javascript; charset=utf-8"),
        "views/mesh.js" => (ASSET_VIEW_MESH.as_bytes(), "application/javascript; charset=utf-8"),
        "views/events.js" => (ASSET_VIEW_EVENTS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/config.js" => (ASSET_VIEW_CONFIG.as_bytes(), "application/javascript; charset=utf-8"),
        "views/requests.js" => (ASSET_VIEW_REQUESTS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/backends.js" => (ASSET_VIEW_BACKENDS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/proxy.js" => (ASSET_VIEW_PROXY.as_bytes(), "application/javascript; charset=utf-8"),
        "views/handoffs.js" => (ASSET_VIEW_HANDOFFS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/logs.js" => (ASSET_VIEW_LOGS.as_bytes(), "application/javascript; charset=utf-8"),
        "views/health.js" => (ASSET_VIEW_HEALTH.as_bytes(), "application/javascript; charset=utf-8"),
        "views/cluster.js" => (ASSET_VIEW_CLUSTER.as_bytes(), "application/javascript; charset=utf-8"),
        _ => return Err(DashboardError::NotFound(format!("asset not found: {path}"))),
    };
    Ok(([(header::CONTENT_TYPE, mime)], content).into_response())
}

async fn fallback_handler(uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        DashboardError::NotFound(format!("endpoint not found: {}", uri.path())).into_response()
    } else {
        (StatusCode::NOT_FOUND, Html(ASSET_404)).into_response()
    }
}
