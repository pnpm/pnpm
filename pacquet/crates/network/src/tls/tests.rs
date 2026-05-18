//! Unit tests for the [`super`] TLS types.

use super::{PerRegistryTls, RegistryTls, TlsConfig, TlsError, strip_port};
use std::collections::HashMap;
use std::net::Ipv4Addr;

fn registry_with(ca: Option<&str>, cert: Option<&str>, key: Option<&str>) -> RegistryTls {
    RegistryTls {
        ca: ca.map(String::from),
        cert: cert.map(String::from),
        key: key.map(String::from),
    }
}

fn build_map(entries: &[(&str, RegistryTls)]) -> PerRegistryTls {
    let mut map = HashMap::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), value.clone());
    }
    PerRegistryTls::from_map(map)
}

#[test]
fn tls_config_default_is_empty() {
    let cfg = TlsConfig::default();
    assert!(cfg.ca.is_empty(), "default CA list is empty");
    assert!(cfg.cert.is_none());
    assert!(cfg.key.is_none());
    assert!(cfg.strict_ssl.is_none(), "default is None — true is applied at build site");
    assert!(cfg.local_address.is_none());
}

#[test]
fn tls_config_clone_round_trip() {
    let cfg = TlsConfig {
        ca: vec!["pem1".to_string(), "pem2".to_string()],
        cert: Some("cert".to_string()),
        key: Some("key".to_string()),
        strict_ssl: Some(false),
        local_address: Some(Ipv4Addr::new(192, 168, 1, 100).into()),
    };
    assert_eq!(cfg.clone(), cfg);
}

#[test]
fn tls_error_invalid_ca_includes_index_in_display() {
    let err = TlsError::InvalidCa { index: 3, reason: "bad pem".into() };
    let rendered = err.to_string();
    assert!(rendered.contains("entry 3"), "expected `entry 3` in {rendered}");
    assert!(rendered.contains("bad pem"), "expected reason in {rendered}");
}

#[test]
fn tls_error_invalid_client_identity_includes_reason_in_display() {
    let err = TlsError::InvalidClientIdentity { reason: "garbage key".into() };
    let rendered = err.to_string();
    assert!(rendered.contains("garbage key"), "expected reason in {rendered}");
}

// --- PerRegistryTls / pick_for_url tests ---

#[test]
fn per_registry_tls_default_is_empty() {
    let m = PerRegistryTls::default();
    assert!(m.is_empty());
    assert!(m.pick_for_url("https://example.com/").is_none());
}

#[test]
fn per_registry_from_map_drops_empty_entries() {
    // pnpm's `configByUri[uri].tls` only materializes when a field
    // gets set, so an empty `RegistryTls` shouldn't take up a slot
    // either. Otherwise the lookup would return a hit that yields
    // an all-`None` override — defeating the point of falling back
    // to top-level fields.
    let m = build_map(&[
        ("//keep.example/", registry_with(Some("ca"), None, None)),
        ("//drop.example/", RegistryTls::default()),
    ]);
    assert!(m.get("//keep.example/").is_some(), "non-empty entry survived");
    assert!(m.get("//drop.example/").is_none(), "empty entry was dropped");
}

#[test]
fn pick_for_url_exact_match_wins() {
    // Step 1 of pickSettingByUrl: an exact URL match short-circuits
    // before nerf-darting kicks in. This lets a user pin a specific
    // tarball URL's TLS without affecting the rest of the registry.
    let exact = "https://registry.example.com/pkg/-/pkg-1.0.0.tgz";
    let m = build_map(&[
        (exact, registry_with(Some("exact-ca"), None, None)),
        ("//registry.example.com/", registry_with(Some("registry-ca"), None, None)),
    ]);
    assert_eq!(m.pick_for_url(exact), Some(exact));
}

#[test]
fn pick_for_url_nerf_dart_match() {
    // Step 2: with no exact match, the nerf-darted URL hits the
    // `//host/` key.
    let m = build_map(&[("//registry.example.com/", registry_with(Some("ca"), None, None))]);
    assert_eq!(m.pick_for_url("https://registry.example.com/pkg"), Some("//registry.example.com/"));
}

#[test]
fn pick_for_url_shorter_path_prefix() {
    // Step 4: a `//host/scope/` key matches any URL under that
    // path, mirroring pnpm's nerf-dart prefix walk.
    let m = build_map(&[(
        "//registry.example.com/scope/",
        registry_with(Some("scope-ca"), None, None),
    )]);
    assert_eq!(
        m.pick_for_url("https://registry.example.com/scope/pkg/-/pkg-1.tgz"),
        Some("//registry.example.com/scope/"),
    );
}

#[test]
fn pick_for_url_strips_port_on_retry() {
    // Steps 3 + 5: a `//host/` key matches a URL that carries an
    // explicit port. The port-strip retry walks back through the
    // earlier steps.
    let m = build_map(&[("//registry.example.com/", registry_with(Some("ca"), None, None))]);
    assert_eq!(
        m.pick_for_url("https://registry.example.com:8443/pkg"),
        Some("//registry.example.com/"),
    );
}

#[test]
fn pick_for_url_misses_when_host_differs() {
    let m = build_map(&[("//registry.example.com/", registry_with(Some("ca"), None, None))]);
    assert_eq!(m.pick_for_url("https://other.example.org/pkg"), None);
}

#[test]
fn pick_for_url_misses_when_path_doesnt_share_prefix() {
    // `//host/foo/` shouldn't match a URL under `/bar/`.
    let m = build_map(&[("//registry.example.com/foo/", registry_with(Some("ca"), None, None))]);
    assert_eq!(m.pick_for_url("https://registry.example.com/bar/pkg"), None);
}

#[test]
fn registry_tls_is_empty_when_all_none() {
    assert!(RegistryTls::default().is_empty());
    assert!(!registry_with(Some("x"), None, None).is_empty());
    assert!(!registry_with(None, Some("x"), None).is_empty());
    assert!(!registry_with(None, None, Some("x")).is_empty());
}

#[test]
fn strip_port_handles_common_shapes() {
    for (input, expected) in [
        ("https://reg.com:8080/path", "https://reg.com/path"),
        ("https://reg.com:8080/", "https://reg.com/"),
        ("https://reg.com:8080", "https://reg.com/"),
        ("https://reg.com/path", "https://reg.com/path"),
        ("https://user:pw@reg.com:8080/path", "https://user:pw@reg.com/path"),
        ("https://[::1]:8080/path", "https://[::1]/path"),
        ("https://[::1]/path", "https://[::1]/path"),
    ] {
        let got = strip_port(input);
        assert_eq!(got, expected, "input={input}");
    }
}
