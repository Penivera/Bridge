use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use cluster::PeerNode;
use proxy::{ShutdownAck, ShutdownReason};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{error, info, warn};

use crate::core::config::{resolve_env_str, DnsHandoffConfig};

/// Action taken during a DNS record synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsUpdateAction {
    /// DNS record already matched target IP; no modification needed.
    Unchanged,
    /// DNS record was found and updated (PATCH).
    Updated,
    /// DNS record did not exist and was newly created (POST).
    Created,
}

/// Result of a successful Cloudflare DNS synchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsUpdateResult {
    pub record_id: String,
    pub record_name: String,
    pub ip: IpAddr,
    pub action: DnsUpdateAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareDnsRecord {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub ttl: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct CloudflareApiError {
    #[serde(default)]
    pub code: Option<i64>,
    pub message: String,
}

fn format_errors(errors: Vec<CloudflareApiError>) -> String {
    errors
        .into_iter()
        .map(|e| match e.code {
            Some(code) => format!("{code}: {}", e.message),
            None => e.message,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Deserialize)]
struct CloudflareListResponse {
    pub success: bool,
    #[serde(default)]
    pub errors: Vec<CloudflareApiError>,
    #[serde(default)]
    pub result: Vec<CloudflareDnsRecord>,
}

#[derive(Debug, Clone, Deserialize)]
struct CloudflareSingleResponse {
    pub success: bool,
    #[serde(default)]
    pub errors: Vec<CloudflareApiError>,
    pub result: Option<CloudflareDnsRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct CloudflareUpdateRecordPayload<'a> {
    pub content: &'a str,
    pub ttl: u32,
    pub proxied: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CloudflareCreateRecordPayload<'a> {
    pub name: &'a str,
    #[serde(rename = "type")]
    pub record_type: &'a str,
    pub content: &'a str,
    pub ttl: u32,
    pub proxied: bool,
}

/// Manages Cloudflare DNS API updates for Ingress DNS Failover (Tier 2).
#[derive(Clone)]
pub struct DnsManager {
    config: DnsHandoffConfig,
    local_node_id: String,
    default_ip: IpAddr,
    client: reqwest::Client,
}

impl DnsManager {
    /// Creates a new `DnsManager` for the specified node and DNS configuration.
    pub fn new(
        config: DnsHandoffConfig,
        local_node_id: impl Into<String>,
        default_ip: IpAddr,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            config,
            local_node_id: local_node_id.into(),
            default_ip,
            client,
        }
    }

    /// Returns a reference to the active DNS handoff configuration.
    pub fn config(&self) -> &DnsHandoffConfig {
        &self.config
    }

    /// Returns the effective target IP address (either explicit override or default node endpoint IP).
    pub fn effective_target_ip(&self) -> IpAddr {
        self.config.target_ip.unwrap_or(self.default_ip)
    }

    /// Synchronizes the configured DNS record to point to the effective target IP.
    pub async fn sync_dns(&self) -> Result<DnsUpdateResult, Box<dyn std::error::Error + Send + Sync>> {
        let raw_token = self
            .config
            .api_token
            .as_deref()
            .ok_or("Cloudflare DNS api_token is required")?;
        let api_token = resolve_env_str(raw_token);

        let raw_zone = self
            .config
            .zone_id
            .as_deref()
            .ok_or("Cloudflare DNS zone_id is required")?;
        let zone_id = resolve_env_str(raw_zone);

        let record_name = self
            .config
            .record_name
            .as_deref()
            .ok_or("Cloudflare DNS record_name is required")?;

        let target_ip = self.effective_target_ip();
        let target_ip_str = target_ip.to_string();
        let record_type = match target_ip {
            IpAddr::V4(_) => "A",
            IpAddr::V6(_) => "AAAA",
        };

        let base_url = self.config.api_base_url.trim_end_matches('/');

        // 1. If explicit record_id is provided, PATCH directly
        if let Some(record_id) = &self.config.record_id {
            let update_url = format!("{base_url}/zones/{zone_id}/dns_records/{record_id}");
            let payload = CloudflareUpdateRecordPayload {
                content: &target_ip_str,
                ttl: self.config.ttl,
                proxied: self.config.proxied,
            };

            let resp = self
                .client
                .patch(&update_url)
                .bearer_auth(&api_token)
                .json(&payload)
                .send()
                .await?;

            let body: CloudflareSingleResponse = resp.json().await?;
            if !body.success {
                let err_msg = format_errors(body.errors);
                return Err(format!("Cloudflare API error updating DNS record: {err_msg}").into());
            }

            return Ok(DnsUpdateResult {
                record_id: record_id.clone(),
                record_name: record_name.to_string(),
                ip: target_ip,
                action: DnsUpdateAction::Updated,
            });
        }

        // 2. Query Cloudflare API to locate record by name and type
        let list_url = format!(
            "{base_url}/zones/{zone_id}/dns_records?name={record_name}&type={record_type}"
        );

        let resp = self
            .client
            .get(&list_url)
            .bearer_auth(&api_token)
            .send()
            .await?;

        let list_body: CloudflareListResponse = resp.json().await?;
        if !list_body.success {
            let err_msg = format_errors(list_body.errors);
            return Err(format!("Cloudflare API error listing DNS records: {err_msg}").into());
        }

        // 3. If record exists, update if changed
        if let Some(existing) = list_body.result.into_iter().next() {
            if existing.content == target_ip_str
                && existing.proxied == self.config.proxied
                && existing.ttl == self.config.ttl
            {
                info!(
                    record_name,
                    record_id = %existing.id,
                    target_ip = %target_ip_str,
                    "Cloudflare DNS record is already pointing to target IP"
                );
                return Ok(DnsUpdateResult {
                    record_id: existing.id,
                    record_name: record_name.to_string(),
                    ip: target_ip,
                    action: DnsUpdateAction::Unchanged,
                });
            }

            let update_url = format!("{base_url}/zones/{zone_id}/dns_records/{}", existing.id);
            let payload = CloudflareUpdateRecordPayload {
                content: &target_ip_str,
                ttl: self.config.ttl,
                proxied: self.config.proxied,
            };

            let resp = self
                .client
                .patch(&update_url)
                .bearer_auth(&api_token)
                .json(&payload)
                .send()
                .await?;

            let body: CloudflareSingleResponse = resp.json().await?;
            if !body.success {
                let err_msg = format_errors(body.errors);
                return Err(format!("Cloudflare API error updating DNS record: {err_msg}").into());
            }

            info!(
                record_name,
                record_id = %existing.id,
                target_ip = %target_ip_str,
                "Updated Cloudflare DNS record to target IP"
            );

            Ok(DnsUpdateResult {
                record_id: existing.id,
                record_name: record_name.to_string(),
                ip: target_ip,
                action: DnsUpdateAction::Updated,
            })
        } else {
            // 4. Record does not exist: create it via POST
            let create_url = format!("{base_url}/zones/{zone_id}/dns_records");
            let payload = CloudflareCreateRecordPayload {
                name: record_name,
                record_type,
                content: &target_ip_str,
                ttl: self.config.ttl,
                proxied: self.config.proxied,
            };

            let resp = self
                .client
                .post(&create_url)
                .bearer_auth(&api_token)
                .json(&payload)
                .send()
                .await?;

            let body: CloudflareSingleResponse = resp.json().await?;
            if !body.success {
                let err_msg = format_errors(body.errors);
                return Err(format!("Cloudflare API error creating DNS record: {err_msg}").into());
            }

            let created_id = body
                .result
                .map(|r| r.id)
                .unwrap_or_else(|| "created".to_string());

            info!(
                record_name,
                record_id = %created_id,
                target_ip = %target_ip_str,
                "Created Cloudflare DNS record with target IP"
            );

            Ok(DnsUpdateResult {
                record_id: created_id,
                record_name: record_name.to_string(),
                ip: target_ip,
                action: DnsUpdateAction::Created,
            })
        }
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
            target_ip = %self.effective_target_ip(),
            record_name = ?self.config.record_name,
            "ingress DNS handoff manager running"
        );

        // Check if already elected leader upon startup
        let initial_leader = leader_rx.borrow().clone();
        if let Some(leader) = initial_leader
            && leader.node_id == self.local_node_id {
                info!(
                    node_id = %self.local_node_id,
                    "node is cluster leader on startup; syncing DNS record"
                );
                if let Err(err) = self.sync_dns().await {
                    warn!(%err, "failed to sync DNS record on initial leader promotion");
                }
            }

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("DNS handoff received shutdown signal; stopping manager");
                    break;
                }
                changed = leader_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let current_leader = leader_rx.borrow().clone();
                    let is_leader = current_leader.as_ref().map(|l| l.node_id == self.local_node_id).unwrap_or(false);

                    if is_leader {
                        info!(
                            node_id = %self.local_node_id,
                            "promoted to cluster leader; updating Cloudflare DNS record"
                        );
                        if let Err(err) = self.sync_dns().await {
                            error!(%err, "failed to update Cloudflare DNS record on leader promotion");
                        }
                    }
                }
            }
        }

        if let Some(ack) = ack_tx {
            let _ = ack
                .send(ShutdownAck {
                    subsystem: "dns_handoff".to_string(),
                    details: None,
                })
                .await;
        }

        Ok(())
    }
}
