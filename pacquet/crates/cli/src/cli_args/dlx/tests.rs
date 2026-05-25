use super::{create_cache_key, scopeless, valid_cache_dir};
use std::collections::BTreeMap;

fn registries() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("default".to_string(), "https://registry.npmjs.org/".to_string());
    map
}

#[test]
fn cache_key_is_stable_and_order_independent() {
    let regs = registries();
    let a = create_cache_key(&["cowsay".to_string(), "lodash".to_string()], &regs, &[]);
    let b = create_cache_key(&["lodash".to_string(), "cowsay".to_string()], &regs, &[]);
    assert_eq!(a, b, "cache key must not depend on input order");
    assert_eq!(a.len(), 32, "short hash is 32 hex chars");
}

#[test]
fn cache_key_changes_with_spec_and_allow_build() {
    let regs = registries();
    let base = create_cache_key(&["cowsay".to_string()], &regs, &[]);
    let versioned = create_cache_key(&["cowsay@1.2.3".to_string()], &regs, &[]);
    let with_build = create_cache_key(&["cowsay".to_string()], &regs, &["cowsay".to_string()]);
    assert_ne!(base, versioned, "different specs hash differently");
    assert_ne!(base, with_build, "allow-build changes the key");
}

#[test]
fn scopeless_strips_scope() {
    assert_eq!(scopeless("@scope/pkg"), "pkg");
    assert_eq!(scopeless("plain"), "plain");
    assert_eq!(scopeless("@only-scope"), "@only-scope");
}

#[test]
fn valid_cache_dir_none_for_missing_link() {
    let dir = tempfile::tempdir().expect("temp dir");
    let link = dir.path().join("pkg");
    assert!(valid_cache_dir(&link, 1440).is_none(), "missing link -> no cache");
}

#[cfg(unix)]
#[test]
fn valid_cache_dir_resolves_fresh_symlink() {
    let dir = tempfile::tempdir().expect("temp dir");
    let target = dir.path().join("install");
    std::fs::create_dir(&target).expect("mkdir target");
    let link = dir.path().join("pkg");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    let resolved = valid_cache_dir(&link, 1440).expect("fresh symlink resolves");
    assert_eq!(resolved, std::fs::canonicalize(&target).unwrap());

    // A zero max-age makes any existing link immediately stale.
    assert!(valid_cache_dir(&link, 0).is_none(), "zero TTL -> stale");
}
