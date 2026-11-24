use super::runtime_version_matches;

#[test]
fn version_matches_an_exact_or_prefix_version() {
    assert!(runtime_version_matches("node", "22.14.0", "22.14.0"));
    assert!(runtime_version_matches("node", "22.14.0", "22.14"));
    assert!(runtime_version_matches("bun", "1.2.3", "v1.2"));
    assert!(!runtime_version_matches("deno", "1.2.30", "1.2.3"));
}

#[test]
fn node_version_matches_a_release_channel_and_a_leading_v() {
    assert!(runtime_version_matches("node", "22.0.0-rc.4", "rc/22.0.0-rc.4"));
    assert!(runtime_version_matches("node", "22.14.0", "v22"));
    assert!(!runtime_version_matches("node", "22.0.0", "lts/18"));
}

#[test]
fn an_empty_version_does_not_match_anything() {
    assert!(!runtime_version_matches("node", "22.14.0", ""));
    assert!(!runtime_version_matches("deno", "2.0.0", "  "));
}
