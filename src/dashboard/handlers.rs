use crate::dashboard::error::DashboardError;
use crate::dashboard::DashboardServer;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type HandlerResult = Result<Json<Value>, DashboardError>;

#[derive(Debug, Deserialize)]
pub struct EventsParams {
    page: Option<usize>,
    per_page: Option<usize>,
    #[serde(rename = "type")]
    event_type: Option<String>,
}

pub async fn handle_health(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    Ok(Json(json!({
        "status": "ok",
        "uptime_secs": server.start_time.elapsed().as_secs()
    })))
}

pub async fn handle_status(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let mut is_leader = false;
    let mut leader_id = None;
    let mut node_count = 1;
    let mut convergence_state = "standalone";
    let mut node_id = "standalone".to_string();

    if let Some(cluster_ctl) = server.cluster.read().await.as_ref() {
        is_leader = cluster_ctl.is_leader().await;
        if let Some(leader) = cluster_ctl.current_leader().await {
            leader_id = Some(leader.node_id.clone());
        }
        let peers = cluster_ctl.peers.read().await;
        node_count = peers.len() + 1;
        node_id = cluster_ctl.local_node.node_id.clone();
        convergence_state = "converged";
    }

    let routes_count = server.registry.len();

    let mut replicas_count = 0;
    if let Some(duplicator) = server.duplicator.read().await.as_ref() {
        replicas_count = duplicator.list_replicas().await.len();
    }

    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": server.start_time.elapsed().as_secs(),
        "node_id": node_id,
        "is_leader": is_leader,
        "is_dashboard_master": is_leader,
        "leader_id": leader_id,
        "node_count": node_count,
        "convergence_state": convergence_state,
        "routes_count": routes_count,
        "replicas_count": replicas_count,
    })))
}

fn node_json(
    node: &cluster::PeerNode,
    is_leader: bool,
    uptime_secs: Option<u64>,
) -> Value {
    let mut v = json!({
        "node_id": node.node_id,
        "mesh_ip": node.mesh_ip,
        "endpoint": node.endpoint,
        "is_leader": is_leader,
        "priority": node.priority,
        "role": if is_leader { "LEADER" } else { "PEER" },
        "health": "healthy",
    });
    if let Some(uptime) = uptime_secs {
        v["uptime_secs"] = json!(uptime);
    }
    v
}

pub async fn handle_nodes(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let mut nodes = Vec::new();
    if let Some(cluster_ctl) = server.cluster.read().await.as_ref() {
        let local_is_leader = cluster_ctl.is_leader().await;
        nodes.push(node_json(
            &cluster_ctl.local_node,
            local_is_leader,
            Some(server.start_time.elapsed().as_secs()),
        ));

        let peers = cluster_ctl.peers.read().await;
        let leader_id = cluster_ctl.current_leader().await.map(|n| n.node_id);

        for (id, peer) in peers.iter() {
            nodes.push(node_json(peer, leader_id.as_ref() == Some(id), None));
        }
    }
    Ok(Json(json!({ "nodes": nodes })))
}

pub async fn handle_node_detail(
    State(server): State<Arc<DashboardServer>>,
    Path(node_id): Path<String>,
) -> HandlerResult {
    if let Some(cluster_ctl) = server.cluster.read().await.as_ref() {
        if cluster_ctl.local_node.node_id == node_id {
            return Ok(Json(node_json(
                &cluster_ctl.local_node,
                cluster_ctl.is_leader().await,
                Some(server.start_time.elapsed().as_secs()),
            )));
        }

        let peers = cluster_ctl.peers.read().await;
        if let Some(peer) = peers.get(&node_id) {
            let is_peer_leader = cluster_ctl
                .current_leader()
                .await
                .is_some_and(|l| l.node_id == node_id);
            return Ok(Json(node_json(peer, is_peer_leader, None)));
        }
    }
    Err(DashboardError::NotFound(format!("Node {node_id} not found")))
}

/// Resolves the liveness of a route target node from current cluster state.
async fn target_health(
    cluster: &std::sync::Arc<tokio::sync::RwLock<Option<std::sync::Arc<cluster::ClusterController>>>>,
    node_id: &str,
) -> &'static str {
    let guard = cluster.read().await;
    let Some(ctrl) = guard.as_ref() else {
        return "unknown";
    };
    if ctrl.local_node.node_id == node_id {
        return "healthy";
    }
    let peers = ctrl.peers.read().await;
    if peers.contains_key(node_id) {
        "healthy"
    } else {
        "dead"
    }
}

pub async fn handle_routes(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let snapshot = server.registry.snapshot();
    let mut routes = Vec::new();
    for (domain, route) in snapshot.iter() {
        let mut targets = Vec::new();
        for t in route.targets.iter() {
            let health = target_health(&server.cluster, &t.node_id).await;
            targets.push(json!({
                "node_id": t.node_id,
                "address": t.address,
                "health": health,
            }));
        }
        let node_health = target_health(&server.cluster, &route.node.node_id).await;
        routes.push(json!({
            "domain": domain,
            "upstream": route.upstream,
            "node_id": route.node.node_id,
            "node_health": node_health,
            "target_addr": route.target_addr(),
            "targets_count": route.targets.len(),
            "has_hash_ring": route.ring.is_some(),
            "targets": targets,
        }));
    }
    Ok(Json(json!({ "routes": routes })))
}

pub async fn handle_route_detail(
    State(server): State<Arc<DashboardServer>>,
    Path(hostname): Path<String>,
) -> HandlerResult {
    if let Some(route) = server.registry.lookup(&hostname) {
        let mut targets = Vec::new();
        for t in route.targets.iter() {
            let health = target_health(&server.cluster, &t.node_id).await;
            targets.push(json!({
                "node_id": t.node_id,
                "address": t.address,
                "health": health,
            }));
        }
        let node_health = target_health(&server.cluster, &route.node.node_id).await;
        return Ok(Json(json!({
            "domain": hostname,
            "upstream": route.upstream,
            "node_id": route.node.node_id,
            "node_health": node_health,
            "target_addr": route.target_addr(),
            "targets_count": route.targets.len(),
            "has_hash_ring": route.ring.is_some(),
            "targets": targets,
        })));
    }
    Err(DashboardError::NotFound(format!("Route {hostname} not found")))
}

pub async fn handle_mesh(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let mut nodes = Vec::new();
    let mut connections = Vec::new();
    let mut local_node_id = String::new();

    if let Some(cluster_ctl) = server.cluster.read().await.as_ref() {
        local_node_id = cluster_ctl.local_node.node_id.clone();
        let local_is_leader = cluster_ctl.is_leader().await;

        nodes.push(node_json(&cluster_ctl.local_node, local_is_leader, None));

        let peers = cluster_ctl.peers.read().await;
        let leader_opt = cluster_ctl.current_leader().await;

        for (id, peer) in peers.iter() {
            let is_peer_leader = leader_opt
                .as_ref()
                .is_some_and(|l| l.node_id == *id);
            nodes.push(node_json(peer, is_peer_leader, None));

            connections.push(json!({
                "from": local_node_id,
                "to": id,
            }));
        }
    }
    Ok(Json(json!({
        "local_node_id": local_node_id,
        "nodes": nodes,
        "connections": connections,
    })))
}

pub async fn handle_events(
    State(server): State<Arc<DashboardServer>>,
    Query(params): Query<EventsParams>,
) -> HandlerResult {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).clamp(1, 200);

    let events = server.events.read().await;
    let filtered: Vec<_> = events
        .iter()
        .rev()
        .filter(|e| {
            params
                .event_type
                .as_ref()
                .is_none_or(|t| e.event_type.eq_ignore_ascii_case(t))
        })
        .cloned()
        .collect();

    let total = filtered.len();
    let page_items: Vec<_> = filtered
        .into_iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .collect();

    Ok(Json(json!({
        "events": page_items,
        "total": total,
        "page": page,
        "per_page": per_page,
    })))
}

pub async fn handle_handoff(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let config = server.config_json.read().await;
    let mode = config
        .as_ref()
        .and_then(|cfg| cfg.get("handoff"))
        .and_then(|h| h.get("mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("none")
        .to_lowercase();

    Ok(Json(json!({ "mode": mode })))
}

pub async fn handle_config(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let config = server.config_json.read().await;
    let doc = config.as_ref().cloned().unwrap_or_else(|| json!({}));
    Ok(Json(json!({ "config": redact_auth_hashes(doc) })))
}

/// Strips password hashes from the configuration view so credential
/// material is never returned through API endpoints.
pub fn redact_auth_hashes(mut doc: Value) -> Value {
    if let Some(auth) = doc.get_mut("auth")
        && let Some(users) = auth.get_mut("users").and_then(|u| u.as_array_mut())
    {
        for user in users {
            if let Some(obj) = user.as_object_mut()
                && obj.contains_key("password_hash")
            {
                obj.insert("password_hash".to_string(), json!("***"));
            }
        }
    }
    doc
}

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    username: String,
    password: String,
}

/// Authenticates a dashboard user and issues a session cookie.
pub async fn handle_login(
    State(server): State<Arc<DashboardServer>>,
    payload: Result<Json<LoginPayload>, axum::extract::rejection::JsonRejection>,
) -> Result<axum::response::Response, DashboardError> {
    let Json(creds) =
        payload.map_err(|e| DashboardError::BadRequest(format!("invalid JSON body: {e}")))?;

    match server.auth.login(&creds.username, &creds.password).await {
        Ok(token) => {
            let cookie = format!(
                "{}={}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}",
                crate::dashboard::SESSION_COOKIE,
                token,
                crate::auth::SESSION_TTL.as_secs()
            );
            let body = json!({ "authenticated": true, "username": creds.username }).to_string();
            let mut response = axum::response::Response::new(axum::body::Body::from(body));
            *response.status_mut() = axum::http::StatusCode::OK;
            response.headers_mut().insert(
                axum::http::header::SET_COOKIE,
                axum::http::HeaderValue::from_str(&cookie)
                    .map_err(|e| DashboardError::Internal(e.to_string()))?,
            );
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            Ok(response)
        }
        Err(_) => {
            let body = json!({ "code": "INVALID_CREDENTIALS", "error": "invalid username or password" })
                .to_string();
            let mut response = axum::response::Response::new(axum::body::Body::from(body));
            *response.status_mut() = axum::http::StatusCode::UNAUTHORIZED;
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            Ok(response)
        }
    }
}

/// Invalidates the session cookie and the in-memory session.
pub async fn handle_logout(
    State(server): State<Arc<DashboardServer>>,
    headers: axum::http::HeaderMap,
) -> Json<Value> {
    if let Some(token) = crate::dashboard::session_token_from_headers(&headers) {
        server.auth.logout(&token).await;
    }
    Json(json!({ "authenticated": false }))
}

/// Reports the current session state for the UI.
pub async fn handle_auth(
    State(server): State<Arc<DashboardServer>>,
    headers: axum::http::HeaderMap,
) -> Json<Value> {
    if let Some(token) = crate::dashboard::session_token_from_headers(&headers)
        && let Some(username) = server.auth.validate(&token).await
    {
        Json(json!({ "authenticated": true, "username": username }))
    } else {
        Json(json!({ "authenticated": false, "username": null }))
    }
}

pub async fn handle_metrics(State(server): State<Arc<DashboardServer>>) -> HandlerResult {
    let snapshot = server.registry.metrics_snapshot();
    let mut routes = serde_json::Map::new();
    for (host, m) in snapshot {
        let error_rate = if m.requests > 0 {
            m.errors as f64 / m.requests as f64
        } else {
            0.0
        };
        routes.insert(
            host,
            json!({
                "requests": m.requests,
                "errors": m.errors,
                "error_rate": error_rate,
                "p50_ms": m.p50_ms,
                "p95_ms": m.p95_ms,
            }),
        );
    }
    Ok(Json(json!({
        "routes": routes,
        "uptime_secs": server.start_time.elapsed().as_secs(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct LogsParams {
    level: Option<String>,
    limit: Option<usize>,
}

pub async fn handle_logs(
    State(_server): State<Arc<DashboardServer>>,
    Query(params): Query<LogsParams>,
) -> HandlerResult {
    let limit = params.limit.unwrap_or(200).clamp(1, 1000);
    let level_filter = params.level.map(|l| l.to_uppercase());

    let logs = crate::core::telemetry::recent_logs();
    let filtered: Vec<_> = logs
        .into_iter()
        .filter(|l| {
            level_filter
                .as_ref()
                .is_none_or(|f| l.level.eq_ignore_ascii_case(f))
        })
        .collect();
    let total = filtered.len();
    // Return the most recent `limit` entries in chronological order.
    let page: Vec<_> = filtered.into_iter().skip(total.saturating_sub(limit)).collect();

    Ok(Json(json!({ "logs": page, "total": total })))
}

pub async fn handle_put_config(
    State(server): State<Arc<DashboardServer>>,
    headers: axum::http::HeaderMap,
    body: String,
) -> HandlerResult {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let doc: Value = if content_type.contains("json") {
        serde_json::from_str(&body)
            .map_err(|e| DashboardError::BadRequest(format!("invalid JSON body: {e}")))?
    } else {
        toml::from_str(&body)
            .map_err(|e| DashboardError::BadRequest(format!("invalid TOML body: {e}")))?
    };

    if !doc.is_object() {
        return Err(DashboardError::Validation(
            "configuration document must be a table/object".into(),
        ));
    }

    let path = server.config_path.read().await.clone().ok_or_else(|| {
        DashboardError::Conflict(
            "no configuration file loaded (running on defaults); changes cannot be persisted"
                .into(),
        )
    })?;

    let is_yaml = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml")
    );

    let serialized = if is_yaml {
        serde_yaml::to_string(&doc)
            .map_err(|e| DashboardError::Validation(format!("cannot encode as YAML: {e}")))?
    } else {
        toml::to_string(&doc)
            .map_err(|e| DashboardError::Validation(format!("cannot encode as TOML: {e}")))?
    };

    // Reject documents the daemon itself would refuse to load.
    if is_yaml {
        crate::core::config::Config::from_yaml_str(&serialized)
            .map_err(|e| DashboardError::Validation(format!("invalid configuration: {e}")))?;
    } else {
        crate::core::config::Config::from_toml_str(&serialized)
            .map_err(|e| DashboardError::Validation(format!("invalid configuration: {e}")))?;
    }

    // Atomic-ish write: same-directory temp file + rename.
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &serialized)
        .map_err(|e| DashboardError::Internal(format!("failed to write config: {e}")))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        DashboardError::Internal(format!("failed to replace config: {e}"))
    })?;

    *server.config_json.write().await = Some(doc.clone());

    Ok(Json(json!({
        "success": true,
        "config": doc,
        "restart_required": true,
        "message": "Configuration saved. Some changes require a daemon restart to take effect.",
    })))
}

pub async fn handle_replicate(
    State(server): State<Arc<DashboardServer>>,
    payload: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> HandlerResult {
    let Json(v) = payload.map_err(|e| DashboardError::BadRequest(format!("invalid JSON body: {e}")))?;
    let node_id = v
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DashboardError::BadRequest("missing node_id".into()))?;

    if let Some(duplicator) = server.duplicator.read().await.as_ref() {
        match duplicator.trigger_replication(node_id).await {
            Ok(count) => {
                let msg = format!("Replicated {count} routes to {node_id}");
                return Ok(Json(json!({
                    "success": true,
                    "target_node": node_id,
                    "spawned_count": count,
                    "message": msg,
                })));
            }
            Err(e) => return Err(DashboardError::Internal(e)),
        }
    }
    Err(DashboardError::Unavailable("duplicator not available".into()))
}

pub async fn handle_failback(
    State(server): State<Arc<DashboardServer>>,
    payload: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> HandlerResult {
    let Json(v) = payload.map_err(|e| DashboardError::BadRequest(format!("invalid JSON body: {e}")))?;
    let domain = v
        .get("domain")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DashboardError::BadRequest("missing domain".into()))?;

    if let Some(duplicator) = server.duplicator.read().await.as_ref() {
        match duplicator.trigger_failback(domain).await {
            Ok(success) => {
                let msg = format!("Failback triggered for domain {domain}");
                return Ok(Json(json!({
                    "success": success,
                    "domain": domain,
                    "message": msg,
                })));
            }
            Err(e) => return Err(DashboardError::Internal(e)),
        }
    }
    Err(DashboardError::Unavailable("duplicator not available".into()))
}
