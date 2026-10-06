//! In-memory dashboard authentication.
//!
//! Credentials (Argon2 PHC hashes) live in the BRIDGE configuration file;
//! sessions and the parsed user table exist only in memory and are lost on
//! daemon restart. No database or persistent session store is involved.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::password_hash::phc::PasswordHash;
use argon2::Argon2;
use rand::Rng;
use tokio::sync::RwLock;

use crate::core::config::AuthConfig;

/// Session lifetime before re-authentication is required.
pub const SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum sessions held in memory per daemon run.
const MAX_SESSIONS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    InvalidPasswordHash(String),
    UserAlreadyExists(String),
    UserNotFound(String),
    UsernameInvalid,
    PasswordTooShort,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "invalid username or password"),
            AuthError::InvalidPasswordHash(msg) => write!(f, "invalid stored password hash: {msg}"),
            AuthError::UserAlreadyExists(u) => write!(f, "user '{u}' already exists"),
            AuthError::UserNotFound(u) => write!(f, "user '{u}' does not exist"),
            AuthError::UsernameInvalid => write!(f, "username must be 1-64 characters, letters, digits, '-', '_' or '.'"),
            AuthError::PasswordTooShort => write!(f, "password must be at least 8 characters"),
        }
    }
}

impl std::error::Error for AuthError {}

struct Session {
    username: String,
    expires_at: Instant,
}

/// Runtime authentication state: password-hash table plus in-memory sessions.
#[derive(Clone)]
pub struct AuthManager {
    enabled: bool,
    users: Arc<RwLock<HashMap<String, String>>>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl AuthManager {
    /// Builds the runtime auth state from the persisted configuration.
    pub fn new(config: &AuthConfig) -> Self {
        let users: HashMap<String, String> = config
            .users
            .iter()
            .filter(|u| !u.username.is_empty())
            .map(|u| (u.username.clone(), u.password_hash.clone()))
            .collect();
        Self {
            enabled: config.enabled,
            users: Arc::new(RwLock::new(users)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Whether authentication is enforced for the dashboard.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether at least one user is configured.
    pub async fn has_users(&self) -> bool {
        !self.users.read().await.is_empty()
    }

    /// Usernames in deterministic order.
    pub async fn list_users(&self) -> Vec<String> {
        let mut names: Vec<String> = self.users.read().await.keys().cloned().collect();
        names.sort();
        names
    }

    /// Verifies credentials and issues a session token on success.
    pub async fn login(&self, username: &str, password: &str) -> Result<String, AuthError> {
        let stored_hash = {
            let users = self.users.read().await;
            users.get(username).cloned()
        };
        let Some(stored_hash) = stored_hash else {
            // Burn comparable work to avoid trivial username enumeration timing.
            let _ = Argon2::default().verify_password(b"placeholder", dummy_hash());
            return Err(AuthError::InvalidCredentials);
        };

        let parsed = PasswordHash::new(&stored_hash)
            .map_err(|_| AuthError::InvalidPasswordHash(username.to_string()))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::InvalidCredentials)?;

        let token = generate_token();
        {
            let mut sessions = self.sessions.write().await;
            if sessions.len() >= MAX_SESSIONS {
                sessions.retain(|_, s| s.expires_at > Instant::now());
            }
            sessions.insert(
                token.clone(),
                Session {
                    username: username.to_string(),
                    expires_at: Instant::now() + SESSION_TTL,
                },
            );
        }
        Ok(token)
    }

    /// Returns the username bound to a valid, unexpired session token.
    pub async fn validate(&self, token: &str) -> Option<String> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(token)?;
        if session.expires_at <= Instant::now() {
            return None;
        }
        Some(session.username.clone())
    }

    /// Invalidates a session token.
    pub async fn logout(&self, token: &str) {
        self.sessions.write().await.remove(token);
    }

    /// Hashes a new password into a PHC string.
    pub fn hash_password(password: &str) -> Result<String, AuthError> {
        validate_password(password)?;
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|_| AuthError::PasswordTooShort)
    }

    /// Registers or replaces a user in the runtime table.
    pub async fn upsert_user(&self, username: &str, password_hash: String) -> Result<(), AuthError> {
        validate_username(username)?;
        self.users
            .write()
            .await
            .insert(username.to_string(), password_hash);
        Ok(())
    }

    /// Replaces the whole runtime user table (used when the config is reloaded).
    pub async fn reload(&self, config: &AuthConfig) {
        let users: HashMap<String, String> = config
            .users
            .iter()
            .filter(|u| !u.username.is_empty())
            .map(|u| (u.username.clone(), u.password_hash.clone()))
            .collect();
        *self.users.write().await = users;
    }

    /// Snapshots the current user/hash table (usernames only for serialization).
    pub async fn user_entries(&self) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = self.users.read().await.clone().into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn dummy_hash() -> &'static PasswordHash {
    static DUMMY: std::sync::OnceLock<PasswordHash> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| {
        Argon2::default()
            .hash_password(b"bridge-dummy-password")
            .expect("dummy argon2 hash")
    })
}

pub fn validate_username(username: &str) -> Result<(), AuthError> {
    let valid = !username.is_empty()
        && username.len() <= 64
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(AuthError::UsernameInvalid)
    }
}

pub fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.len() < 8 {
        return Err(AuthError::PasswordTooShort);
    }
    Ok(())
}

/// Applies a user's password hash to the `[auth]` section of a raw config
/// document. Returns the updated document and whether the user was created.
pub fn apply_user_to_config_doc(
    mut doc: serde_json::Value,
    username: &str,
    password_hash: &str,
) -> (serde_json::Value, bool) {
    let Some(auth) = doc.as_object_mut().and_then(|root| {
        root.entry("auth")
            .or_insert_with(|| serde_json::json!({ "enabled": true, "users": [] }))
            .as_object_mut()
    }) else {
        return (doc, false);
    };

    let Some(users) = auth
        .entry("users")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
    else {
        return (doc, false);
    };

    let mut created = false;
    if let Some(user) = users
        .iter_mut()
        .find(|u| u.get("username").and_then(|v| v.as_str()) == Some(username))
    {
        if let Some(obj) = user.as_object_mut() {
            obj.insert(
                "password_hash".to_string(),
                serde_json::json!(password_hash),
            );
        }
    } else {
        users.push(serde_json::json!({
            "username": username,
            "password_hash": password_hash,
        }));
        created = true;
    }

    (doc, created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_generation_is_unique_and_hex() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn username_validation() {
        assert!(validate_username("admin").is_ok());
        assert!(validate_username("user-1").is_ok());
        assert!(validate_username("a.b_c").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("has space").is_err());
        assert!(validate_username("x".repeat(65).as_str()).is_err());
    }

    #[test]
    fn password_validation() {
        assert!(validate_password("s3cure-pass").is_ok());
        assert!(validate_password("short").is_err());
    }
}
