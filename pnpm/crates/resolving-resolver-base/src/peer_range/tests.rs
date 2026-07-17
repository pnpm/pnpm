use super::{get_peer_version_range, is_acceptable_peer_spec, is_valid_peer_range};

#[test]
fn is_valid_peer_range_only_accepts_semver_and_workspace_catalog() {
    assert!(is_valid_peer_range("^1.0.0"));
    assert!(is_valid_peer_range("workspace:^"));
    assert!(is_valid_peer_range("catalog:"));
    assert!(!is_valid_peer_range("work:5.x.x"));
    assert!(!is_valid_peer_range("npm:bar@^5"));
    assert!(!is_valid_peer_range("file:../foo"));
}

#[test]
fn is_acceptable_peer_spec_accepts_scheme_carrying_specifiers() {
    assert!(is_acceptable_peer_spec("^1.0.0"));
    assert!(is_acceptable_peer_spec("workspace:^"));
    assert!(is_acceptable_peer_spec("catalog:"));
    assert!(is_acceptable_peer_spec("work:5.x.x"));
    assert!(is_acceptable_peer_spec("npm:bar@^5"));
    assert!(is_acceptable_peer_spec("file:../foo"));
    assert!(is_acceptable_peer_spec("git+https://example.com/foo.git"));
}

#[test]
fn is_acceptable_peer_spec_rejects_bare_name_at_version_typos() {
    assert!(!is_acceptable_peer_spec("bar@1.2.3"));
    assert!(!is_acceptable_peer_spec("latest"));
    assert!(!is_acceptable_peer_spec("not a range"));
}

#[test]
fn get_peer_version_range_keeps_valid_peer_ranges() {
    assert_eq!(get_peer_version_range("^1.0.0"), "^1.0.0");
    assert_eq!(get_peer_version_range(">=1.2.3 || ^3.2.1"), ">=1.2.3 || ^3.2.1");
    assert_eq!(get_peer_version_range("catalog:"), "catalog:");
}

#[test]
fn get_peer_version_range_strips_workspace_prefix() {
    assert_eq!(get_peer_version_range("workspace:^"), "^");
    assert_eq!(get_peer_version_range("workspace:1.2.3"), "1.2.3");
    assert_eq!(get_peer_version_range("workspace:*"), "*");
}

#[test]
fn get_peer_version_range_extracts_named_registry_and_npm_bodies() {
    assert_eq!(get_peer_version_range("work:5.x.x"), "5.x.x");
    assert_eq!(get_peer_version_range("work:^5.0.0"), "^5.0.0");
    assert_eq!(get_peer_version_range("npm:bar@^5"), "^5");
    assert_eq!(get_peer_version_range("npm:@scope/bar@~2.1.0"), "~2.1.0");
    assert_eq!(get_peer_version_range("npm:^5.0.0"), "^5.0.0");
}

#[test]
fn get_peer_version_range_falls_back_to_star() {
    assert_eq!(get_peer_version_range("file:../foo"), "*");
    assert_eq!(get_peer_version_range("link:../foo"), "*");
    assert_eq!(get_peer_version_range("git+https://example.com/foo.git"), "*");
    assert_eq!(get_peer_version_range("npm:bar"), "*");
    assert_eq!(get_peer_version_range("work:@scope/bar"), "*");
}
