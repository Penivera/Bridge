use std::fs;
use std::path::Path;
use std::process::Command;
use serde::Deserialize;

pub const DEFAULT_REPO: &str = "Penivera/Bridge";

/// Metadata representing a GitHub release.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<GitHubAsset>,
}

/// Metadata representing a release asset file.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// Information returned by an update check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    pub current_version: String,
    pub target_platform: &'static str,
    pub latest_tag: String,
    pub is_newer: bool,
    pub release: GitHubRelease,
}

/// Detects the target platform tuple matching Bridge's release assets.
pub fn detect_target() -> Result<&'static str, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        _ => Err(format!(
            "unsupported platform '{os}/{arch}'. In-place auto-update is currently supported on Linux (x86_64, aarch64)."
        )),
    }
}

/// Compares current version with a release tag (e.g. `0.1.0` vs `v0.1.0-8f7998b5` or `v0.2.0`).
pub fn is_update_available(current_version: &str, tag_name: &str) -> bool {
    let clean_current = current_version.trim_start_matches('v').trim();
    let clean_tag = tag_name.trim_start_matches('v').trim();
    if clean_current == clean_tag {
        return false;
    }
    true
}

/// Computes the SHA256 hex digest of data using `ring`.
pub fn compute_sha256(data: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Parses a sha256sums.txt manifest to find the expected digest for `asset_name`.
pub fn parse_checksum_manifest(manifest: &str, asset_name: &str) -> Option<String> {
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let filename = parts[parts.len() - 1].trim_start_matches('*');
            if filename == asset_name || filename.ends_with(&format!("/{asset_name}")) {
                return Some(parts[0].to_lowercase());
            }
        }
    }
    None
}

/// Queries GitHub for latest release or specific release tag.
pub async fn fetch_release(
    client: &reqwest::Client,
    repo: &str,
    tag: Option<&str>,
) -> Result<GitHubRelease, Box<dyn std::error::Error + Send + Sync>> {
    let url = match tag {
        Some(t) => {
            let clean_tag = if t.starts_with('v') { t.to_string() } else { format!("v{t}") };
            format!("https://api.github.com/repos/{repo}/releases/tags/{clean_tag}")
        }
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };

    let user_agent = format!("bridge-updater/{}", env!("CARGO_PKG_VERSION"));
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, user_agent)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API request to {url} failed with status {status}: {body}").into());
    }

    let release: GitHubRelease = resp.json().await?;
    Ok(release)
}

/// Checks whether an update is available without modifying any files.
pub async fn check_update(
    client: &reqwest::Client,
    repo: &str,
    version_tag: Option<&str>,
) -> Result<UpdateCheck, Box<dyn std::error::Error + Send + Sync>> {
    let target = detect_target()?;
    let release = fetch_release(client, repo, version_tag).await?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let is_newer = is_update_available(&current_version, &release.tag_name);

    Ok(UpdateCheck {
        current_version,
        target_platform: target,
        latest_tag: release.tag_name.clone(),
        is_newer,
        release,
    })
}

/// Downloads, verifies, and installs an update into the current executable location.
pub async fn perform_update(
    client: &reqwest::Client,
    repo: &str,
    version_tag: Option<&str>,
    force: bool,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let check = check_update(client, repo, version_tag).await?;

    if !check.is_newer && !force && version_tag.is_none() {
        return Ok(format!(
            "Bridge is already up to date (current: v{}, release: {}). Use --force to reinstall.",
            check.current_version, check.latest_tag
        ));
    }

    let target = check.target_platform;

    // 1. Find matching target asset
    let tar_asset = check
        .release
        .assets
        .iter()
        .find(|a| a.name.contains(target) && a.name.ends_with(".tar.gz"))
        .ok_or_else(|| {
            format!(
                "Release {} does not provide a binary asset for target '{}'",
                check.latest_tag, target
            )
        })?;

    // 2. Find checksum asset
    let checksum_asset = check
        .release
        .assets
        .iter()
        .find(|a| a.name == "sha256sums.txt" || a.name == format!("{}.sha256", tar_asset.name));

    // 3. Download tar archive
    println!("Downloading {} ({} bytes)...", tar_asset.name, tar_asset.size);
    let user_agent = format!("bridge-updater/{}", check.current_version);
    let tar_bytes = client
        .get(&tar_asset.browser_download_url)
        .header(reqwest::header::USER_AGENT, &user_agent)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // 4. Verify checksum
    let actual_sha = compute_sha256(&tar_bytes);
    println!("Computed SHA256: {}", actual_sha);

    if let Some(c_asset) = checksum_asset {
        println!("Verifying against checksum manifest: {}...", c_asset.name);
        let c_text = client
            .get(&c_asset.browser_download_url)
            .header(reqwest::header::USER_AGENT, &user_agent)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let expected_sha = if c_asset.name.ends_with(".sha256") {
            c_text.split_whitespace().next().map(|s| s.to_string())
        } else {
            parse_checksum_manifest(&c_text, &tar_asset.name)
        };

        match expected_sha {
            Some(expected) => {
                if expected.to_lowercase() != actual_sha.to_lowercase() {
                    return Err(format!(
                        "SHA256 checksum mismatch for {}!\n  Expected: {}\n  Actual:   {}\nAborting update to prevent executing corrupted or untrusted code.",
                        tar_asset.name, expected, actual_sha
                    ).into());
                }
                println!("Checksum verified successfully.");
            }
            None => {
                return Err(format!(
                    "No checksum entry found for {} in {}; refusing unverified install",
                    tar_asset.name, c_asset.name
                ).into());
            }
        }
    } else {
        println!("Warning: No sha256sums.txt found in release; proceeding with caution.");
    }

    // 5. Extract to temporary directory
    let temp_dir = std::env::temp_dir().join(format!("bridge-update-{}", std::process::id()));
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    fs::create_dir_all(&temp_dir)?;

    let archive_path = temp_dir.join(&tar_asset.name);
    fs::write(&archive_path, &tar_bytes)?;

    let extract_status = Command::new("tar")
        .args(["-xzf", archive_path.to_str().unwrap(), "-C", temp_dir.to_str().unwrap()])
        .status()?;

    if !extract_status.success() {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err("Failed to extract tar archive using 'tar -xzf'".into());
    }

    let extracted_bridge = temp_dir.join("bridge");
    if !extracted_bridge.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
        return Err("Archive did not contain a 'bridge' executable binary".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&extracted_bridge, perms)?;
    }

    // 6. Test extracted binary before replacing
    let test_run = Command::new(&extracted_bridge).arg("version").output();
    match test_run {
        Ok(output) if output.status.success() => {
            let ver_out = String::from_utf8_lossy(&output.stdout);
            println!("Extracted binary self-test passed: {}", ver_out.trim());
        }
        Ok(output) => {
            let _ = fs::remove_dir_all(&temp_dir);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Extracted binary failed self-test (exit code {:?}): {}", output.status.code(), stderr).into());
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(format!("Extracted binary could not be executed: {e}").into());
        }
    }

    // 7. Atomic in-place replacement
    let current_exe = std::env::current_exe()?;
    let canonical_exe = current_exe.canonicalize().unwrap_or(current_exe.clone());

    println!("Installing updated binary to {}...", canonical_exe.display());

    let target_parent = canonical_exe.parent().unwrap_or(Path::new("/usr/local/bin"));
    let temp_replacement = target_parent.join(format!(".bridge.tmp.{}", std::process::id()));

    match fs::copy(&extracted_bridge, &temp_replacement) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(format!(
                "Permission denied writing to '{}'. Root privileges are required.\nPlease re-run: sudo bridge update",
                target_parent.display()
            ).into());
        }
        Err(err) => {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(format!("Failed to copy binary to temporary file: {err}").into());
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(&temp_replacement, perms);
    }

    // Atomic replace via rename
    if let Err(err) = fs::rename(&temp_replacement, &canonical_exe) {
        let _ = fs::remove_file(&temp_replacement);
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(format!("Failed to replace binary at '{}': {err}", canonical_exe.display()).into());
    }

    // Clean up temp dir
    let _ = fs::remove_dir_all(&temp_dir);

    // 8. Service reload check
    let service_restarted = restart_systemd_service_if_active();

    let mut result_msg = format!("Bridge successfully updated to {} at {}", check.latest_tag, canonical_exe.display());
    if service_restarted {
        result_msg.push_str("\nRestarted active systemd service: bridge");
    }

    Ok(result_msg)
}

fn restart_systemd_service_if_active() -> bool {
    let check = Command::new("systemctl")
        .args(["is-active", "--quiet", "bridge"])
        .status();

    if let Ok(st) = check && st.success() {
        println!("Detected active systemd service 'bridge', triggering restart...");
        let restart = Command::new("systemctl")
            .args(["restart", "bridge"])
            .status();
        return restart.map(|s| s.success()).unwrap_or(false);
    }
    false
}
