use proxy::core::enums::{ProxyMode, Scheme};

#[test]
fn test_proxy_mode_defaults_and_variants() {
    assert_eq!(ProxyMode::default(), ProxyMode::Handoff);
    assert_ne!(ProxyMode::Direct, ProxyMode::Handoff);
    assert_ne!(ProxyMode::Managed, ProxyMode::Handoff);
    assert_ne!(ProxyMode::Managed, ProxyMode::Direct);
}

#[test]
fn test_scheme_ports() {
    assert_eq!(Scheme::default(), Scheme::Https);
    assert_eq!(Scheme::Http.port(), 80);
    assert_eq!(Scheme::Https.port(), 443);
}
