use super::{Duration, Identity, cached_resolution, lockfile, store_resolution};
use pnpr_auth::AuthState;
use pnpr_route::Footprint;

#[tokio::test]
async fn a_resolution_cached_under_one_graph_is_not_served_under_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pnpr.yaml");
    std::fs::write(
        &path,
        "storage: ./store\nregistries:\n  npmjs:\n    type: upstream\n    url: https://registry.npmjs.org/\n    public: true\ndefaultRegistry: npmjs\n",
    )
    .unwrap();
    let config = pnpr_fixtures::load_seeded_config(&path, "127.0.0.1:0".parse().unwrap(), None);
    let state = crate::server::state_and_router(config, AuthState::in_memory(), None).unwrap().0;
    let ttl = Duration::from_mins(1);
    let before = state.inner.current_routing();
    let engine = state.test_resolver(&before);
    let key = engine.graph_cache_key("request");
    let store = |key: &str| {
        store_resolution(
            &engine.cache.entries,
            ttl,
            key.to_string(),
            Footprint::default(),
            b"secret",
            &lockfile("1.0.0"),
        )
    };
    assert!(store(&key));
    let cached = |key: &str, engine: &crate::resolver::Resolver| {
        cached_resolution(
            &engine.cache.entries,
            ttl,
            key,
            &engine.route_context,
            &Identity::Anonymous,
        )
        .is_some()
    };
    assert!(cached(&key, engine));

    state.test_replace_routing();
    let after = state.inner.current_routing();
    let next_engine = state.test_resolver(&after);
    let next_key = next_engine.graph_cache_key("request");
    assert!(!cached(&key, next_engine), "the swap clears the results");
    assert!(store(&key), "a resolution still running under the old graph finishes");
    assert!(!cached(&next_key, next_engine), "and stays out of the new graph's key");
}

#[tokio::test]
async fn forget_results_clears_verdicts_and_rotates_cache_generation() {
    use crate::resolver::verdict_cache::{VerdictLookup, VerdictRecord};

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pnpr.yaml");
    std::fs::write(
        &path,
        "storage: ./store\nregistries:\n  npmjs:\n    type: upstream\n    url: https://registry.npmjs.org/\n    public: true\ndefaultRegistry: npmjs\n",
    )
    .unwrap();
    let config = pnpr_fixtures::load_seeded_config(&path, "127.0.0.1:0".parse().unwrap(), None);
    let state = crate::server::state_and_router(config, AuthState::in_memory(), None).unwrap().0;
    let ttl = Duration::from_mins(1);
    let routing = state.inner.current_routing();
    let engine = state.test_resolver(&routing);
    let before_key = engine.graph_cache_key("request");

    assert!(store_resolution(
        &engine.cache.entries,
        ttl,
        before_key.clone(),
        Footprint::default(),
        b"secret",
        &lockfile("1.0.0"),
    ));
    let verdicts = engine.cache.verdicts.as_ref().expect("verdict cache opened");
    let public_footprint = Footprint::default();
    verdicts.record(VerdictRecord {
        hash: "hash-1",
        footprint: &public_footprint,
        secret: &engine.cache.secret,
        epoch: &engine.local_registry.verdict_epoch(),
        policy: serde_json::Map::new(),
    });
    let lookup = VerdictLookup {
        hash: "hash-1",
        epoch: &engine.local_registry.verdict_epoch(),
        secret: &engine.cache.secret,
        route_context: &engine.route_context,
        identity: &Identity::Anonymous,
    };
    assert!(verdicts.verified_footprint(&lookup, |_| true).is_some());

    state.test_forget_resolver_results();

    let after_key = engine.graph_cache_key("request");
    assert_ne!(before_key, after_key, "forget_results advances the cache generation");
    assert!(
        cached_resolution(
            &engine.cache.entries,
            ttl,
            &before_key,
            &engine.route_context,
            &Identity::Anonymous,
        )
        .is_none(),
    );
    assert!(verdicts.verified_footprint(&lookup, |_| true).is_none());

    // An in-flight resolution that pinned `before_key` before `forget_results`
    // cannot populate `after_key` when it finishes.
    assert!(store_resolution(
        &engine.cache.entries,
        ttl,
        before_key,
        Footprint::default(),
        b"secret",
        &lockfile("1.0.0"),
    ));
    assert!(
        cached_resolution(
            &engine.cache.entries,
            ttl,
            &after_key,
            &engine.route_context,
            &Identity::Anonymous,
        )
        .is_none(),
    );
}
