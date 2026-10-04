use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use registry::{DomainRegistry, Route};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::core::config::UdpServiceConfig;

/// The reason triggering a shutdown event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Sigint,
    Sigterm,
    Manual,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShutdownReason::Sigint => write!(f, "SIGINT (Ctrl+C)"),
            ShutdownReason::Sigterm => write!(f, "SIGTERM"),
            ShutdownReason::Manual => write!(f, "Manual"),
        }
    }
}

/// Acknowledgment sent back by a subsystem once it has finalized its state
/// and updated any necessary files before yielding control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownAck {
    pub subsystem: String,
    pub details: Option<String>,
}

/// Full snapshot of runtime state captured during shutdown or checkpointing.
/// Includes all statically configured and dynamically/runtime-discovered routes and services.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeState {
    pub timestamp: u64,
    pub shutdown_reason: String,
    pub proxy_mode: String,
    pub routes_count: usize,
    /// Maps domain -> Route (including target nodes, direct/mesh endpoints, routing preferences, and upstream overrides)
    pub routes: HashMap<String, Route>,
    /// All active UDP proxy services (including dynamic or statically configured)
    pub udp_services: Vec<UdpServiceConfig>,
    pub status: String,
}

/// Coordinates graceful shutdown across Bridge subsystems using a broadcast channel
/// for sending the shutdown signal and an mpsc channel for receiving completion acks.
pub struct ShutdownCoordinator {
    broadcast_tx: broadcast::Sender<ShutdownReason>,
    ack_tx: mpsc::Sender<ShutdownAck>,
    ack_rx: mpsc::Receiver<ShutdownAck>,
    expected_subsystems: HashSet<String>,
    state_file_path: Option<PathBuf>,
}

impl ShutdownCoordinator {
    /// Creates a new `ShutdownCoordinator` with a default broadcast capacity of 16.
    pub fn new(state_file_path: Option<PathBuf>) -> Self {
        let (broadcast_tx, _) = broadcast::channel(16);
        let (ack_tx, ack_rx) = mpsc::channel(32);
        Self {
            broadcast_tx,
            ack_tx,
            ack_rx,
            expected_subsystems: HashSet::new(),
            state_file_path,
        }
    }

    /// Registers a subsystem by name, returning a broadcast receiver for the shutdown signal
    /// and an mpsc sender to signal back when state finalization is complete.
    pub fn register_subsystem(
        &mut self,
        name: &str,
    ) -> (broadcast::Receiver<ShutdownReason>, mpsc::Sender<ShutdownAck>) {
        self.expected_subsystems.insert(name.to_string());
        (self.broadcast_tx.subscribe(), self.ack_tx.clone())
    }

    /// Returns a sender handle to broadcast shutdown signals manually.
    pub fn sender(&self) -> broadcast::Sender<ShutdownReason> {
        self.broadcast_tx.clone()
    }

    /// Returns the path to the configured state file, if any.
    pub fn state_file_path(&self) -> Option<&PathBuf> {
        self.state_file_path.as_ref()
    }

    /// Listens asynchronously for OS signals: SIGINT (Ctrl+C) and SIGTERM.
    pub async fn wait_for_signal() -> ShutdownReason {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .expect("failed to register SIGTERM signal listener");
            tokio::select! {
                res = tokio::signal::ctrl_c() => {
                    if let Err(e) = res {
                        tracing::error!("error listening for ctrl_c: {e}");
                    }
                    ShutdownReason::Sigint
                }
                _ = sigterm.recv() => {
                    ShutdownReason::Sigterm
                }
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl_c");
            ShutdownReason::Sigint
        }
    }

    /// Broadcasts the shutdown signal to all registered receivers and awaits their
    /// acknowledgment signals on the mpsc channel before yielding control.
    pub async fn broadcast_and_wait(
        &mut self,
        reason: ShutdownReason,
        timeout_duration: Duration,
    ) -> Vec<ShutdownAck> {
        tracing::info!(reason = %reason, "broadcasting shutdown signal to all subsystems");
        let _ = self.broadcast_tx.send(reason);

        let mut acks = Vec::new();
        let expected_count = self.expected_subsystems.len();

        if expected_count == 0 {
            return acks;
        }

        let deadline = tokio::time::sleep(timeout_duration);
        tokio::pin!(deadline);

        while acks.len() < expected_count {
            tokio::select! {
                Some(ack) = self.ack_rx.recv() => {
                    tracing::info!(
                        subsystem = %ack.subsystem,
                        details = ?ack.details,
                        "subsystem finalized state and signaled back"
                    );
                    acks.push(ack);
                }
                _ = &mut deadline => {
                    tracing::warn!(
                        received = acks.len(),
                        expected = expected_count,
                        "shutdown ack timeout reached before all subsystems responded"
                    );
                    break;
                }
            }
        }

        tracing::info!(
            acks_received = acks.len(),
            expected = expected_count,
            "all responding subsystems finished finalization; yielding control"
        );

        acks
    }
}

/// Spawns the state finalizer task that listens for the shutdown broadcast,
/// flushes runtime state, discovered routes, and services into the designated state file on disk (JSON or YAML),
/// and signals back via the mpsc acknowledgment channel.
pub fn spawn_state_finalizer(
    state_file_path: PathBuf,
    mode_str: String,
    registry: Arc<DomainRegistry>,
    udp_services: Vec<UdpServiceConfig>,
    mut shutdown_rx: broadcast::Receiver<ShutdownReason>,
    ack_tx: mpsc::Sender<ShutdownAck>,
) {
    tokio::spawn(async move {
        if let Ok(reason) = shutdown_rx.recv().await {
            tracing::info!(
                reason = %reason,
                path = %state_file_path.display(),
                "state finalizer: persisting runtime state, discovered routes, and services to disk"
            );

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Snapshot all routes at the moment of shutdown (capturing any runtime-discovered routes)
            let routes_snapshot = (*registry.snapshot()).clone();
            let routes_count = routes_snapshot.len();

            let state = RuntimeState {
                timestamp: now,
                shutdown_reason: reason.to_string(),
                proxy_mode: mode_str,
                routes_count,
                routes: routes_snapshot,
                udp_services,
                status: "gracefully_stopped".to_string(),
            };

            // Detect format based on extension: .yaml / .yml -> YAML, otherwise JSON
            let is_yaml = state_file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
                .unwrap_or(false);

            let serialized_res = if is_yaml {
                serde_yaml::to_string(&state).map_err(|e| e.to_string())
            } else {
                serde_json::to_string_pretty(&state).map_err(|e| e.to_string())
            };

            let details = match serialized_res {
                Ok(content) => match tokio::fs::write(&state_file_path, content).await {
                    Ok(_) => {
                        tracing::debug!(
                            path = %state_file_path.display(),
                            routes_count,
                            "runtime state and discovered routes successfully saved"
                        );
                        Some(format!(
                            "state written to {} ({} routes, {} udp services)",
                            state_file_path.display(),
                            routes_count,
                            state.udp_services.len()
                        ))
                    }
                    Err(err) => {
                        tracing::error!(path = %state_file_path.display(), "failed to write state file: {err}");
                        Some(format!("write error: {err}"))
                    }
                },
                Err(err) => {
                    tracing::error!(path = %state_file_path.display(), "failed to serialize state: {err}");
                    Some(format!("serialize error: {err}"))
                }
            };

            let _ = ack_tx
                .send(ShutdownAck {
                    subsystem: "state_finalizer".to_string(),
                    details,
                })
                .await;
        }
    });
}
