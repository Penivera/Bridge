use bridge::updater::{
    compute_sha256, detect_target, is_update_available, parse_checksum_manifest, GitHubRelease,
};

#[test]
fn test_detect_target_linux() {
    let target = detect_target();
    if cfg!(target_os = "linux") {
        assert!(target.is_ok());
        let t = target.unwrap();
        assert!(t == "x86_64-unknown-linux-musl" || t == "aarch64-unknown-linux-musl");
    }
}

#[test]
fn test_compute_sha256_known_vector() {
    // SHA256 of empty string: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let empty_hash = compute_sha256(b"");
    assert_eq!(
        empty_hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    // SHA256 of "hello bridge"
    let test_hash = compute_sha256(b"hello bridge");
    assert_eq!(test_hash.len(), 64);
}

#[test]
fn test_parse_checksum_manifest_formats() {
    let manifest = r#"
# Release Checksums
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  bridge-v0.1.0-x86_64-unknown-linux-musl.tar.gz
11223344556677889900aabbccddeeff0011223344556677889900aabbccddeeff *bridge-v0.1.0-aarch64-unknown-linux-musl.tar.gz
aabbccddeeff0011223344556677889900aabbccddeeff00112233445566778899 dist/bridge-v0.2.0-x86_64-unknown-linux-musl.tar.gz
"#;

    let res1 = parse_checksum_manifest(manifest, "bridge-v0.1.0-x86_64-unknown-linux-musl.tar.gz");
    assert_eq!(
        res1,
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
    );

    let res2 = parse_checksum_manifest(
        manifest,
        "bridge-v0.1.0-aarch64-unknown-linux-musl.tar.gz",
    );
    assert_eq!(
        res2,
        Some("11223344556677889900aabbccddeeff0011223344556677889900aabbccddeeff".to_string())
    );

    let res3 = parse_checksum_manifest(manifest, "bridge-v0.2.0-x86_64-unknown-linux-musl.tar.gz");
    assert_eq!(
        res3,
        Some("aabbccddeeff0011223344556677889900aabbccddeeff00112233445566778899".to_string())
    );

    let res_none = parse_checksum_manifest(manifest, "nonexistent.tar.gz");
    assert_eq!(res_none, None);
}

#[test]
fn test_is_update_available() {
    assert!(!is_update_available("0.1.0", "0.1.0"));
    assert!(!is_update_available("0.1.0", "v0.1.0"));
    assert!(!is_update_available("v0.1.0", "0.1.0"));
    assert!(!is_update_available("v0.1.0", "v0.1.0"));

    assert!(is_update_available("0.1.0", "v0.2.0"));
    assert!(is_update_available("0.1.0", "v0.1.0-8f7998b5"));
    assert!(is_update_available("0.1.0", "0.2.0"));
}

#[test]
fn test_github_release_deserialization() {
    let json_data = r#"{
        "tag_name": "v0.1.0-abcdef12",
        "name": "BRIDGE v0.1.0",
        "published_at": "2026-10-08T18:00:00Z",
        "assets": [
            {
                "name": "bridge-v0.1.0-abcdef12-x86_64-unknown-linux-musl.tar.gz",
                "browser_download_url": "https://github.com/Penivera/Bridge/releases/download/v0.1.0-abcdef12/bridge-v0.1.0-abcdef12-x86_64-unknown-linux-musl.tar.gz",
                "size": 12345678
            },
            {
                "name": "sha256sums.txt",
                "browser_download_url": "https://github.com/Penivera/Bridge/releases/download/v0.1.0-abcdef12/sha256sums.txt",
                "size": 256
            }
        ]
    }"#;

    let release: GitHubRelease = serde_json::from_str(json_data).expect("failed to deserialize GitHubRelease");
    assert_eq!(release.tag_name, "v0.1.0-abcdef12");
    assert_eq!(release.assets.len(), 2);
    assert_eq!(
        release.assets[0].name,
        "bridge-v0.1.0-abcdef12-x86_64-unknown-linux-musl.tar.gz"
    );
    assert_eq!(release.assets[1].name, "sha256sums.txt");
}
