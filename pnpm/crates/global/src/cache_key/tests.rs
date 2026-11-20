use super::create_global_cache_key;

#[test]
fn order_independent_for_aliases_and_registries() {
    let registries = vec![("default".to_string(), "https://registry.npmjs.org/".to_string())];
    let forward = create_global_cache_key(&["foo".to_string(), "bar".to_string()], &registries);
    let reversed = create_global_cache_key(&["bar".to_string(), "foo".to_string()], &registries);
    assert_eq!(forward, reversed);
}

#[test]
fn differs_by_alias_set() {
    let registries = vec![("default".to_string(), "https://registry.npmjs.org/".to_string())];
    let foo_key = create_global_cache_key(&["foo".to_string()], &registries);
    let bar_key = create_global_cache_key(&["bar".to_string()], &registries);
    assert_ne!(foo_key, bar_key);
}

#[test]
fn order_independent_across_multiple_registries() {
    let forward = create_global_cache_key(
        &["foo".to_string()],
        &[
            ("default".to_string(), "https://registry.npmjs.org/".to_string()),
            ("@scope".to_string(), "https://npm.example.com/".to_string()),
        ],
    );
    let reordered = create_global_cache_key(
        &["foo".to_string()],
        &[
            ("@scope".to_string(), "https://npm.example.com/".to_string()),
            ("default".to_string(), "https://registry.npmjs.org/".to_string()),
        ],
    );
    assert_eq!(forward, reordered);
}

/// Golden hash pinning the exact byte format pnpm hashes, computed from
/// `sha256(JSON.stringify([["is-positive"],[["default","https://registry.npmjs.org/"]]]))`.
/// A wrong-but-deterministic payload shape would change this digest, so this
/// guards parity with pnpm's `createGlobalCacheKey`.
#[test]
fn matches_pnpm_golden_hash() {
    let key = create_global_cache_key(
        &["is-positive".to_string()],
        &[("default".to_string(), "https://registry.npmjs.org/".to_string())],
    );
    assert_eq!(key, "cc470234bbcb66c2acfea612ed1e7d7bb33d4cd7e49da589a68a705d07472e1b");
}
