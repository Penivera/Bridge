use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use proxy::core::enums::ProxyMode;
use registry::{DomainRegistry, Node, Route};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// Inbound control requests sent over the Unix domain socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IpcRequest {
    Ping,
    Status,
    ListRoutes,
    AddRoute {
        domain: String,
        #[serde(default)]
        upstream: Option<SocketAddr>,
        #[serde(default)]
        node_id: Option<String>,
    },
    RemoveRoute {
        domain: String,
    },
}

/// Outbound control responses sent back over the Unix domain socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok {
        #[serde(flatten)]
        data: IpcData,
    },
    Error {
        message: String,
    },
}

/// Payload contained inside an `IpcResponse::Ok`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum IpcData {
    Pong {
        message: String,
    },
    Status {
        version: String,
        uptime_secs: u64,
        routes_count: usize,
        proxy_mode: String,
    },
    Routes {
        routes: HashMap<String, RouteInfo>,
    },
    RouteResult {
        domain: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        registered: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        removed: Option<bool>,
    },
    Message {
        message: String,
    },
}

/// Serializable representation of a route returned over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteInfo {
    pub upstream: Option<SocketAddr>,
    pub node_id: String,
    pub target_addr: SocketAddr,
}

impl From<&Route> for RouteInfo {
    fn from(r: &Route) -> Self {
        Self {
            upstream: r.upstream,
            node_id: r.node.node_id.clone(),
            target_addr: r.target_addr(),
        }
    }
}

/// Unix Domain Socket IPC server for local host and container control.
pub struct IpcServer {
    socket_path: PathBuf,
    registry: Arc<DomainRegistry>,
    proxy_mode: ProxyMode,
    start_time: Instant,
}

impl IpcServer {
    /// Creates a new IPC server configuration.
    pub fn new(
        socket_path: impl AsRef<Path>,
        registry: Arc<DomainRegistry>,
        proxy_mode: ProxyMode,
    ) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            registry,
            proxy_mode,
            start_time: Instant::now(),
        }
    }

    /// Resolves and binds the Unix domain socket.
    ///
    /// Automatically removes stale socket files before binding and creates parent directories.
    pub fn bind(socket_path: impl AsRef<Path>) -> std::io::Result<UnixListener> {
        let path = socket_path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        UnixListener::bind(path)
    }

    /// Runs the IPC listener loop with a simple broadcast receiver.
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

    /// Runs the IPC listener loop coordinating with Bridge's ShutdownCoordinator.
    pub async fn run_with_shutdown(
        self,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<proxy::ShutdownReason>,
        ack_tx: Option<tokio::sync::mpsc::Sender<proxy::ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (listener, active_path) = match Self::bind(&self.socket_path) {
            Ok(l) => (l, self.socket_path.clone()),
            Err(err) => {
                tracing::warn!(
                    path = %self.socket_path.display(),
                    %err,
                    "failed to bind preferred IPC socket, trying /tmp/bridge.sock fallback"
                );
                let fallback = PathBuf::from("/tmp/bridge.sock");
                let l = Self::bind(&fallback)?;
                (l, fallback)
            }
        };

        tracing::info!(
            socket = %active_path.display(),
            "IPC control server listening"
        );

        let registry = self.registry.clone();
        let proxy_mode = self.proxy_mode;
        let start_time = self.start_time;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::info!("IPC server received shutdown signal");
                    break;
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _)) => {
                            let reg = registry.clone();
                            tokio::spawn(async move {
                                if let Err(err) = handle_ipc_connection(stream, reg, proxy_mode, start_time).await {
                                    tracing::debug!(%err, "IPC client connection closed with error");
                                }
                            });
                        }
                        Err(err) => {
                            tracing::warn!(%err, "error accepting IPC connection");
                        }
                    }
                }
            }
        }

        // Clean up socket file on graceful shutdown
        if active_path.exists() {
            let _ = std::fs::remove_file(&active_path);
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(proxy::ShutdownAck {
                    subsystem: "ipc_server".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}

async fn handle_ipc_connection(
    stream: UnixStream,
    registry: Arc<DomainRegistry>,
    proxy_mode: ProxyMode,
    start_time: Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<IpcRequest>(trimmed) {
            Ok(request) => match request {
                IpcRequest::Ping => IpcResponse::Ok {
                    data: IpcData::Pong {
                        message: "pong".to_string(),
                    },
                },
                IpcRequest::Status => IpcResponse::Ok {
                    data: IpcData::Status {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        uptime_secs: start_time.elapsed().as_secs(),
                        routes_count: registry.len(),
                        proxy_mode: format!("{:?}", proxy_mode),
                    },
                },
                IpcRequest::ListRoutes => {
                    let snapshot = registry.snapshot();
                    let routes = snapshot
                        .iter()
                        .map(|(k, v)| (k.clone(), RouteInfo::from(v)))
                        .collect();
                    IpcResponse::Ok {
                        data: IpcData::Routes { routes },
                    }
                }
                IpcRequest::AddRoute {
                    domain,
                    upstream,
                    node_id,
                } => {
                    let nid = node_id.unwrap_or_else(|| "self".to_string());
                    let target_addr = upstream.unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 80)));
                    let node = Node::new(nid, target_addr);
                    let route = Route::new(upstream, node);
                    registry.insert(domain.clone(), route);
                    IpcResponse::Ok {
                        data: IpcData::RouteResult {
                            domain,
                            registered: Some(true),
                            removed: None,
                        },
                    }
                }
                IpcRequest::RemoveRoute { domain } => {
                    let removed = registry.remove(&domain);
                    IpcResponse::Ok {
                        data: IpcData::RouteResult {
                            domain,
                            registered: None,
                            removed: Some(removed.is_some()),
                        },
                    }
                }
            },
            Err(err) => IpcResponse::Error {
                message: format!("invalid JSON request: {err}"),
            },
        };

        let mut serialized = serde_json::to_string(&response)?;
        serialized.push('\n');
        writer.write_all(serialized.as_bytes()).await?;
        writer.flush().await?;
    }

    Ok(())
}

/// Client for connecting to the Bridge IPC Unix domain socket.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    /// Connects to a Bridge IPC server listening on `socket_path`.
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let stream = UnixStream::connect(socket_path).await?;
        Ok(Self { stream })
    }

    /// Sends a typed `IpcRequest` and receives an `IpcResponse`.
    pub async fn send(
        &mut self,
        request: &IpcRequest,
    ) -> Result<IpcResponse, Box<dyn std::error::Error>> {
        let mut req_str = serde_json::to_string(request)?;
        req_str.push('\n');
        self.stream.write_all(req_str.as_bytes()).await?;
        self.stream.flush().await?;

        let mut reader = BufReader::new(&mut self.stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: IpcResponse = serde_json::from_str(line.trim())?;
        Ok(response)
    }

    /// Sends a `ping` and returns the pong response string.
    pub async fn ping(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::Ping).await? {
            IpcResponse::Ok {
                data: IpcData::Pong { message },
            } => Ok(message),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Retrieves status from the running daemon.
    pub async fn status(&mut self) -> Result<IpcData, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::Status).await? {
            IpcResponse::Ok { data } => Ok(data),
            IpcResponse::Error { message } => Err(message.into()),
        }
    }

    /// Lists all routes from the running daemon.
    pub async fn list_routes(
        &mut self,
    ) -> Result<HashMap<String, RouteInfo>, Box<dyn std::error::Error>> {
        match self.send(&IpcRequest::ListRoutes).await? {
            IpcResponse::Ok {
                data: IpcData::Routes { routes },
            } => Ok(routes),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Adds a route dynamically.
    pub async fn add_route(
        &mut self,
        domain: impl Into<String>,
        upstream: Option<SocketAddr>,
        node_id: Option<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::AddRoute {
                domain: domain.into(),
                upstream,
                node_id,
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::RouteResult { registered, .. },
            } => Ok(registered.unwrap_or(true)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }

    /// Removes a route dynamically.
    pub async fn remove_route(
        &mut self,
        domain: impl Into<String>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        match self
            .send(&IpcRequest::RemoveRoute {
                domain: domain.into(),
            })
            .await?
        {
            IpcResponse::Ok {
                data: IpcData::RouteResult { removed, .. },
            } => Ok(removed.unwrap_or(false)),
            IpcResponse::Error { message } => Err(message.into()),
            other => Err(format!("unexpected response: {:?}", other).into()),
        }
    }
}
