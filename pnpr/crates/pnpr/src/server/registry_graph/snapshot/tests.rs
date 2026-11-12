use super::{
    super::super::{AppState, routing::state_and_router},
    build_snapshot,
};
use pnpr_auth::AuthState;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_upstream::CacheValidators;
use std::time::Duration;

fn state(dir: &std::path::Path, upstream: &str) -> AppState {
    let path = dir.join("pnpr.yaml");
    let yaml = format!(
        "storage: ./store\nregistries:\n  mirror:\n    type: upstream\n    url: {upstream}\n    public: true\ndefaultRegistry: mirror\n",
    );
    std::fs::write(&path, yaml).unwrap();
    let config = crate::Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    state_and_router(config, AuthState::in_memory(), None).unwrap().0
}

async fn fetch_example(state: &AppState) -> bool {
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let routing = state.routing();
    routing.upstream_clients["mirror"]
        .fetch_packument(&name, &CacheValidators::default())
        .await
        .is_ok()
}

#[tokio::test]
async fn a_rebuild_keeps_the_client_of_an_unchanged_upstream() {
    let mut server = mockito::Server::new_async().await;
    let failing = server
        .mock("GET", "/example")
        .with_status(500)
        .expect(3)
        .create_async()
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let state = state(tmp.path(), &server.url());
    assert!(!fetch_example(&state).await);
    assert!(!fetch_example(&state).await);

    let current = state.routing();
    let unchanged = build_snapshot(
        &state.inner.config,
        (**current).clone(),
        Some(&current),
        &state.inner.upstream_guard_context,
    );
    state.inner.replace_routing(unchanged);
    assert!(!fetch_example(&state).await);

    let current = state.routing();
    let mut retuned = (**current).clone();
    retuned.upstreams["mirror"].requests.timeout = Duration::from_secs(7);
    let changed = build_snapshot(
        &state.inner.config,
        retuned,
        Some(&current),
        &state.inner.upstream_guard_context,
    );
    state.inner.replace_routing(changed);
    assert!(!fetch_example(&state).await);

    failing.assert_async().await;
}
