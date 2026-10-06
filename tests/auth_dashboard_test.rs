use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bridge::auth::AuthManager;
use bridge::cluster::{ClusterController, PeerNode};
use bridge::core::config::{AuthConfig, AuthUser};
use bridge::dashboard::DashboardServer;
use bridge::ipc::{IpcClient, IpcServer};
use bridge::mesh::WireGuardDevice;
use proxy::core::enums::ProxyMode;
use proxy::ShutdownReason;
use registry::DomainRegistry;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, RwLock};

/// Helper to create a test `ClusterController`.
async fn create_test_cluster(
    local_id: &str,
    mesh_octet: u8,
) -> (Arc<ClusterController>, PeerNode) {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local_addr = socket.local_addr().unwrap();
    let mesh_ip = IpAddr::V4(Ipv4Addr::new(10, 8, 0, mesh_octet));
    let peer_node = PeerNode::new(
        local_id,
        format!("pubkey_{local_id}"),
        mesh_ip,
        local_addr,
    );
    let wg = Arc::new(RwLock::new(WireGuardDevice::new(
        "wg0",
        format!("privkey_{local_id}"),
        format!("pubkey_{local_id}"),
        local_addr.port(),
        mesh_ip,
    )));
    let registry = Arc::new(DomainRegistry::new());
    let controller = Arc::new(ClusterController::new_with_socket(
        peer_node.clone(),
        socket,
        wg,
        registry,
    ));
    (controller, peer_node)
}

fn auth_config_with_user(username: &str, password: &str) -> AuthConfig {
    let hash = AuthManager::hash_password(password).unwrap();
    AuthConfig {
        enabled: true,
        users: vec![AuthUser {
            username: username.to_string(),
            password_hash: hash,
        }],
    }
}

/// Spawns a dashboard with auth enabled; returns base URL, auth manager and shutdown.
async fn spawn_auth_dashboard(
    auth: Arc<AuthManager>,
) -> (
    String,
    broadcast::Sender<ShutdownReason>,
    tokio::sync::mpsc::Receiver<proxy::ShutdownAck>,
) {
    let registry = Arc::new(DomainRegistry::new());
    let cluster_slot = Arc::new(RwLock::new(None));
    let duplicator_slot = Arc::new(RwLock::new(None));
    let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tmp_listener.local_addr().unwrap().port();
    drop(tmp_listener);

    let dashboard = Arc::new(DashboardServer::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        registry,
        cluster_slot,
        duplicator_slot,
        auth,
    ));
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (ack_tx, ack_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        let _ = dashboard.run_with_shutdown(shutdown_rx, Some(ack_tx)).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (format!("http://127.0.0.1:{port}"), shutdown_tx, ack_rx)
}

// ---------------------------------------------------------------------------
// 2. Password hashing / verification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_password_hashing_and_verification() {
    let config = auth_config_with_user("admin", "s3cret-password");
    let manager = AuthManager::new(&config);

    // The stored hash is a PHC string, never the plaintext password.
    let entries = manager.user_entries().await;
    assert_eq!(entries.len(), 1);
    assert!(entries[0].1.starts_with("$argon2id$"));
    assert!(!entries[0].1.contains("s3cret-password"));

    // Correct password verifies, wrong password and wrong username do not.
    assert!(manager.login("admin", "s3cret-password").await.is_ok());
    assert!(manager.login("admin", "wrong-password").await.is_err());
    assert!(manager.login("nobody", "s3cret-password").await.is_err());
}

#[tokio::test]
async fn test_runtime_sessions_are_not_persistent() {
    // 9. Runtime state does not require a database: sessions live in memory
    // only; a daemon restart (fresh manager from the same config) invalidates
    // all previously issued tokens.
    let config = auth_config_with_user("admin", "s3cret-password");
    let manager = AuthManager::new(&config);
    let token = manager.login("admin", "s3cret-password").await.unwrap();
    assert!(manager.validate(&token).await.is_some());
    manager.logout(&token).await;
    assert!(manager.validate(&token).await.is_none());

    let restarted = AuthManager::new(&config);
    let token2 = manager.login("admin", "s3cret-password").await.unwrap();
    assert!(restarted.validate(&token2).await.is_none());
    let token3 = restarted.login("admin", "s3cret-password").await.unwrap();
    assert!(restarted.validate(&token3).await.is_some());
}

// ---------------------------------------------------------------------------
// 3. Authentication success / failure over HTTP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dashboard_login_success_and_failure() {
    let auth = Arc::new(AuthManager::new(&auth_config_with_user("admin", "s3cret-password")));
    let (base_url, shutdown_tx, _ack_rx) = spawn_auth_dashboard(auth).await;
    let client = reqwest::Client::new();

    // 7. Unauthenticated access is rejected.
    let unauth = client
        .get(format!("{base_url}/api/v1/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);

    let page = client
        .get(&base_url)
        .send()
        .await
        .unwrap();
    // reqwest follows the middleware's redirect to /login.
    assert!(page.url().path() == "/login", "expected redirect to /login, got {}", page.url());

    // Login page is publicly reachable.
    let login_page = client
        .get(format!("{base_url}/login"))
        .send()
        .await
        .unwrap();
    assert_eq!(login_page.status(), 200);
    assert!(login_page.text().await.unwrap().contains("Sign in"));

    // Wrong credentials → 401.
    let bad = client
        .post(format!("{base_url}/api/v1/login"))
        .json(&serde_json::json!({ "username": "admin", "password": "wrong-password" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 401);

    // Correct credentials → 200 + session cookie.
    let good = client
        .post(format!("{base_url}/api/v1/login"))
        .json(&serde_json::json!({ "username": "admin", "password": "s3cret-password" }))
        .send()
        .await
        .unwrap();
    assert_eq!(good.status(), 200);
    let set_cookie = good.headers().get("set-cookie").unwrap().to_str().unwrap();
    assert!(set_cookie.contains("bridge_session="));
    assert!(set_cookie.contains("HttpOnly"));

    // Authenticated request succeeds with the cookie.
    let authed = client
        .get(format!("{base_url}/api/v1/status"))
        .header("cookie", set_cookie.split(';').next().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(authed.status(), 200);

    // /api/v1/auth reports the session.
    let session = client
        .get(format!("{base_url}/api/v1/auth"))
        .header("cookie", set_cookie.split(';').next().unwrap())
        .send()
        .await
        .unwrap();
    let session_json: serde_json::Value = session.json().await.unwrap();
    assert_eq!(session_json["authenticated"], true);
    assert_eq!(session_json["username"], "admin");

    let _ = shutdown_tx.send(ShutdownReason::Manual);
}

// ---------------------------------------------------------------------------
// 4/5. Leader recognized as dashboard master / follower not master
// ---------------------------------------------------------------------------

async fn spawn_dashboard_with_cluster(
    leader_node_id: Option<&str>,
    local_id: &str,
) -> (String, broadcast::Sender<ShutdownReason>) {
    let registry = Arc::new(DomainRegistry::new());
    let (ctrl, local_peer) = create_test_cluster(local_id, 1).await;
    if let Some(leader) = leader_node_id {
        let leader_peer = PeerNode::new(
            leader,
            "pubkey_other",
            IpAddr::V4(Ipv4Addr::new(10, 8, 0, 2)),
            "127.0.0.1:51999".parse().unwrap(),
        );
        ctrl.election_state.write().await.current_leader = Some(leader_peer);
    } else {
        ctrl.election_state.write().await.current_leader = Some(local_peer.clone());
        ctrl.election_state.write().await.role = bridge::cluster::ElectionRole::Leader;
    }

    let cluster_slot = Arc::new(RwLock::new(Some(ctrl)));
    let duplicator_slot = Arc::new(RwLock::new(None));
    let auth = Arc::new(AuthManager::new(&AuthConfig::default()));
    let tmp_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tmp_listener.local_addr().unwrap().port();
    drop(tmp_listener);

    let dashboard = Arc::new(DashboardServer::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        registry,
        cluster_slot,
        duplicator_slot,
        auth,
    ));
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = dashboard.run_with_shutdown(shutdown_rx, None).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (format!("http://127.0.0.1:{port}"), shutdown_tx)
}

#[tokio::test]
async fn test_leader_recognized_as_dashboard_master() {
    let (base_url, shutdown_tx) = spawn_dashboard_with_cluster(None, "dash-node").await;
    let status: serde_json::Value = reqwest::get(format!("{base_url}/api/v1/status"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["is_leader"], true);
    assert_eq!(status["is_dashboard_master"], true);
    assert_eq!(status["leader_id"], "dash-node");
    let _ = shutdown_tx.send(ShutdownReason::Manual);
}

#[tokio::test]
async fn test_follower_not_recognized_as_dashboard_master() {
    let (base_url, shutdown_tx) =
        spawn_dashboard_with_cluster(Some("other-leader"), "follower-node").await;
    let status: serde_json::Value = reqwest::get(format!("{base_url}/api/v1/status"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["is_leader"], false);
    assert_eq!(status["is_dashboard_master"], false);
    assert_eq!(status["leader_id"], "other-leader");
    let _ = shutdown_tx.send(ShutdownReason::Manual);
}

// ---------------------------------------------------------------------------
// 6. Leadership change updates dashboard ownership (route moves with leader)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_leadership_change_moves_dashboard_ownership() {
    let registry = Arc::new(DomainRegistry::new());
    let dash_addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
    let domain = "dashboard.example.com";

    // No leader yet → no route.
    bridge::daemon::apply_dashboard_ownership(&registry, domain, "node-a", None, dash_addr);
    assert!(registry.lookup(domain).is_none());

    // Node A becomes leader → route exists and points at the local dashboard.
    bridge::daemon::apply_dashboard_ownership(&registry, domain, "node-a", Some("node-a"), dash_addr);
    let route = registry.lookup(domain).expect("route on leader");
    assert_eq!(route.target_addr(), dash_addr);

    // Leadership moves to Node B → this node drops the route.
    bridge::daemon::apply_dashboard_ownership(&registry, domain, "node-a", Some("node-b"), dash_addr);
    assert!(registry.lookup(domain).is_none());

    // Node A re-elected → route returns.
    bridge::daemon::apply_dashboard_ownership(&registry, domain, "node-a", Some("node-a"), dash_addr);
    assert!(registry.lookup(domain).is_some());

    // Sanity: registry still stores no route for followers (no fake state).
    let follower_registry = Arc::new(DomainRegistry::new());
    bridge::daemon::apply_dashboard_ownership(
        &follower_registry,
        domain,
        "node-b",
        Some("node-a"),
        dash_addr,
    );
    assert!(follower_registry.lookup(domain).is_none());
}

// ---------------------------------------------------------------------------
// 8. No password hashes exposed through API endpoints
// ---------------------------------------------------------------------------

#[test]
fn test_redact_auth_hashes() {
    let doc = serde_json::json!({
        "auth": {
            "enabled": true,
            "users": [
                { "username": "admin", "password_hash": "$argon2id$SECRET" },
                { "username": "ops", "password_hash": "$argon2id$SECRET2" },
            ]
        },
        "other": "value"
    });
    let redacted = bridge::dashboard::handlers::redact_auth_hashes(doc);
    assert_eq!(redacted["auth"]["users"][0]["password_hash"], "***");
    assert_eq!(redacted["auth"]["users"][1]["password_hash"], "***");
    assert_eq!(redacted["auth"]["users"][0]["username"], "admin");
    assert_eq!(redacted["other"], "value");

    // Documents without an auth section pass through unchanged.
    let plain = serde_json::json!({ "proxy": { "mode": "Direct" } });
    assert_eq!(
        bridge::dashboard::handlers::redact_auth_hashes(plain.clone()),
        plain
    );
}

#[tokio::test]
async fn test_config_endpoint_never_returns_hashes() {
    let auth = Arc::new(AuthManager::new(&auth_config_with_user("admin", "s3cret-password")));
    let (base_url, shutdown_tx, _ack_rx) = spawn_auth_dashboard(auth).await;
    let client = reqwest::Client::new();

    let login = client
        .post(format!("{base_url}/api/v1/login"))
        .json(&serde_json::json!({ "username": "admin", "password": "s3cret-password" }))
        .send()
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let resp = client
        .get(format!("{base_url}/api/v1/config"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();
    assert!(!body.contains("$argon2id"), "hash leaked in config response: {body}");
    assert!(!body.contains("s3cret-password"));

    let _ = shutdown_tx.send(ShutdownReason::Manual);
}

// ---------------------------------------------------------------------------
// 1. CLI user creation (IPC path the CLI drives) + persistence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ipc_user_create_passwd_and_list() {
    let socket_path = PathBuf::from(format!(
        "/tmp/bridge-test-ipc-users-{}.sock",
        std::process::id()
    ));
    let registry = Arc::new(DomainRegistry::new());
    let ipc_server = IpcServer::new(&socket_path, registry, ProxyMode::Handoff);

    // Config file the daemon "runs from".
    let cfg_path = std::env::temp_dir().join(format!("bridge-users-{}.toml", std::process::id()));
    std::fs::write(
        &cfg_path,
        "[auth]\nenabled = true\n\n[[auth.users]]\nusername = \"existing\"\npassword_hash = \"$argon2id$placeholder\"\n",
    )
    .unwrap();

    let raw = std::fs::read_to_string(&cfg_path).unwrap();
    let doc: serde_json::Value = toml::from_str(&raw).unwrap();
    let auth = Arc::new(AuthManager::new(
        &AuthConfig {
            enabled: true,
            users: vec![AuthUser {
                username: "existing".to_string(),
                password_hash: "$argon2id$placeholder".to_string(),
            }],
        },
    ));
    *ipc_server.auth_handle().write().await = Some(auth.clone());
    *ipc_server.config_view_handle().write().await = Some(doc);
    *ipc_server.config_path_handle().write().await = Some(cfg_path.clone());

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    tokio::spawn(async move {
        let _ = ipc_server.run_with_shutdown(shutdown_rx, None).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path).await.expect("connect");

    // 1. List users (initial).
    let users = client.list_users().await.unwrap();
    assert_eq!(users, vec!["existing".to_string()]);

    // 2. Create a new user.
    let msg = client.create_user("admin", "brand-new-password").await.unwrap();
    assert!(msg.contains("created"));

    // 3. The user was persisted to the config file as an argon2 hash.
    let on_disk = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(on_disk.contains("username = \"admin\""));
    assert!(on_disk.contains("$argon2id$"));
    assert!(!on_disk.contains("brand-new-password"));

    // 4. Duplicate creation is rejected.
    let dup = client.create_user("admin", "another-password").await;
    assert!(dup.is_err());

    // 5. Runtime auth manager accepts the new user immediately.
    assert!(auth.login("admin", "brand-new-password").await.is_ok());

    // 6. passwd updates an existing user; unknown user is rejected.
    let msg = client.set_password("existing", "rotated-password-1").await.unwrap();
    assert!(msg.contains("updated"));
    let unknown = client.set_password("ghost", "rotated-password-1").await;
    assert!(unknown.is_err());

    // 7. The updated hash is persisted.
    let on_disk = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(!on_disk.contains("$argon2id$placeholder"));

    let _ = shutdown_tx.send(ShutdownReason::Manual);
    let _ = std::fs::remove_file(&cfg_path);
    let _ = std::fs::remove_file(socket_path);
}

// ---------------------------------------------------------------------------
// CLI surface parses the user subcommands (thin handler layer over IPC).
// ---------------------------------------------------------------------------

#[test]
fn test_cli_user_subcommands_parse() {
    use bridge::cli::{Args, Commands, UserCommands};
    use clap::Parser;

    let args = Args::try_parse_from(["bridge", "user", "create", "admin"]).unwrap();
    match args.command {
        Some(Commands::User {
            command: UserCommands::Create { username, .. },
        }) => assert_eq!(username, "admin"),
        other => panic!("unexpected command: {other:?}"),
    }

    let args = Args::try_parse_from(["bridge", "user", "passwd", "admin"]).unwrap();
    assert!(matches!(
        args.command,
        Some(Commands::User {
            command: UserCommands::Passwd { .. }
        })
    ));

    let args = Args::try_parse_from(["bridge", "user", "list"]).unwrap();
    assert!(matches!(
        args.command,
        Some(Commands::User {
            command: UserCommands::List { .. }
        })
    ));
}
