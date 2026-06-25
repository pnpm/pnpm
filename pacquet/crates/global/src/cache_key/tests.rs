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
