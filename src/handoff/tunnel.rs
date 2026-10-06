use std::sync::Arc;
use std::time::Duration;

use cluster::PeerNode;
use proxy::{ShutdownAck, ShutdownReason};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tracing::{error, info, warn};

use crate::core::config::{resolve_env_str, TunnelHandoffConfig};

/// Manages the Cloudflare Tunnel (`cloudflared`) child process lifecycle for Ingress Handoff (Tier 3a).
#[derive(Clone)]
pub struct TunnelManager {
    config: TunnelHandoffConfig,
    local_node_id: String,
    child: Arc<Mutex<Option<Child>>>,
}

impl TunnelManager {
    /// Creates a new `TunnelManager` for the specified node and tunnel configuration.
    pub fn new(config: TunnelHandoffConfig, local_node_id: impl Into<String>) -> Self {
        Self {
            config,
            local_node_id: local_node_id.into(),
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns a reference to the active tunnel configuration.
    pub fn config(&self) -> &TunnelHandoffConfig {
        &self.config
    }

    /// Checks whether the `cloudflared` process is currently alive and running.
    pub async fn is_running(&self) -> bool {
        let mut lock = self.child.lock().await;
        if let Some(ref mut child) = *lock {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited
                    *lock = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Spawns the `cloudflared` tunnel process if not already running.
    pub async fn start(&self) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let mut lock = self.child.lock().await;
        if let Some(ref mut child) = *lock
            && let Ok(None) = child.try_wait() {
                return Ok(child.id().unwrap_or(0));
            }

        let mut cmd = Command::new(&self.config.binary_path);

        let is_cloudflared = self.config.binary_path == "cloudflared"
            || self.config.binary_path.ends_with("/cloudflared")
            || self.config.binary_path.ends_with("\\cloudflared.exe");

        if is_cloudflared {
            if let Some(token) = &self.config.token {
                let resolved = resolve_env_str(token);
                cmd.args(["tunnel", "run", "--token", &resolved]);
            } else if let Some(tunnel_id) = &self.config.tunnel_id {
                if let Some(cred_file) = &self.config.credentials_file {
                    cmd.args(["tunnel", "--credentials-file"]);
                    cmd.arg(cred_file);
                    cmd.args(["run", tunnel_id]);
                } else {
                    cmd.args(["tunnel", "run", tunnel_id]);
                }
            } else {
                cmd.args(["tunnel", "run"]);
            }
        }

        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }

        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        *lock = Some(child);
        info!(
            pid,
            node_id = %self.local_node_id,
            "spawned cloudflared tunnel process"
        );
        Ok(pid)
    }

    /// Stops the running `cloudflared` tunnel process with graceful termination timeout.
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let maybe_child = {
            let mut lock = self.child.lock().await;
            lock.take()
        };

        if let Some(mut child) = maybe_child {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_millis(500), child.wait()).await;
            info!(
                node_id = %self.local_node_id,
                "stopped cloudflared tunnel process"
            );
        }
        Ok(())
    }

    /// Runs the reactive leadership watcher loop coordinating with `ClusterController` and `ShutdownCoordinator`.
    pub async fn run_with_shutdown(
        self: Arc<Self>,
        mut leader_rx: watch::Receiver<Option<PeerNode>>,
        mut shutdown_rx: broadcast::Receiver<ShutdownReason>,
        ack_tx: Option<mpsc::Sender<ShutdownAck>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            node_id = %self.local_node_id,
            warm_standby = self.config.warm_standby,
            "ingress tunnel handoff manager running"
        );

        // Warm standby: runs cloudflared continuously across all nodes
        if self.config.warm_standby {
            if let Err(err) = self.start().await {
                warn!(%err, "failed to start cloudflared in warm standby mode");
            }
        } else {
            // Cold standby: check if already elected leader upon startup
            let initial_leader = leader_rx.borrow().clone();
            if let Some(leader) = initial_leader
                && leader.node_id == self.local_node_id {
                    info!(
                        node_id = %self.local_node_id,
                        "node is cluster leader on startup; starting tunnel"
                    );
                    if let Err(err) = self.start().await {
                        warn!(%err, "failed to start cloudflared on initial leader promotion");
                    }
                }
        }

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("tunnel handoff received shutdown signal; stopping cloudflared");
                    let _ = self.stop().await;
                    break;
                }
                changed = leader_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let current_leader = leader_rx.borrow().clone();
                    let is_leader = current_leader.as_ref().map(|l| l.node_id == self.local_node_id).unwrap_or(false);

                    if is_leader {
                        if !self.is_running().await {
                            info!(
                                node_id = %self.local_node_id,
                                "promoted to cluster leader; activating Cloudflare tunnel"
                            );
                            if let Err(err) = self.start().await {
                                error!(%err, "failed to start cloudflared on promotion");
                            }
                        }
                    } else if !self.config.warm_standby && self.is_running().await {
                        info!(
                            node_id = %self.local_node_id,
                            "demoted from cluster leader; deactivating Cloudflare tunnel (cold standby)"
                        );
                        let _ = self.stop().await;
                    }
                }
            }
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(ShutdownAck {
                    subsystem: "tunnel_handoff".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}
