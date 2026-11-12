use super::{
    super::super::routing::state_and_router, PINNED, carry_pinned_routing, serves_without_graph,
};
use pnpr_auth::AuthState;
use std::sync::Arc;

#[tokio::test]
async fn a_pinned_request_keeps_its_snapshot_across_a_swap() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pnpr.yaml");
    std::fs::write(
        &path,
        "storage: ./store\nregistries:\n  local:\n    type: hosted\ndefaultRegistry: local\n",
    )
    .unwrap();
    let config = crate::Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    let state = state_and_router(config, AuthState::in_memory(), None).unwrap().0;
    let pinned = state.routing();

    PINNED.scope(Arc::clone(&pinned), async {
        let mut replacement = (**pinned).clone();
        replacement.hosted.clear();
        state.inner.replace_routing(super::super::build_snapshot(
            &state.inner.config,
            replacement,
            Some(&pinned),
            &state.inner.upstream_guard_context,
        ));
        assert!(Arc::ptr_eq(&state.routing(), &pinned));
    })
    .await;

    assert!(!Arc::ptr_eq(&state.routing(), &pinned));
    assert!(state.routing().hosted.is_empty());
}

#[test]
fn probes_accounts_and_the_registry_admin_api_serve_without_a_graph() {
    for path in [
        "/-/ping",
        "/-/user/org.couchdb.user:root",
        "/npm/-/user/org.couchdb.user:root",
        "/npm/~local/-/whoami",
        "/-/npm/v1/tokens",
        "/-/v1/login",
        "/-/pnpr/v0/admin/users",
        "/-/pnpr/v0/sign-in",
        "/-/oidc/example/login",
        "/-/oidc/example/callback",
        "/-/oidc/handoff",
        "/-/ready",
        "/-/live",
        "/~local/-/ready",
        "/-/pnpr/v0/admin/registries",
        "/-/pnpr/v0/admin/registries/local",
        "/-/pnpr/v0/admin/registry-revisions/3/restore",
    ] {
        assert!(serves_without_graph(path), "{path}");
    }
    for path in [
        "/npm/example",
        "/-/pnpr/v0/admin/rules/npm/local",
        "/-/pnpr/v0/admin/registriesx",
        "/npm/-/org/acme/team",
        "/npmx/-/whoami",
    ] {
        assert!(!serves_without_graph(path), "{path}");
    }
}

#[tokio::test]
async fn work_a_request_spawns_keeps_the_request_snapshot_across_a_swap() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pnpr.yaml");
    std::fs::write(
        &path,
        "storage: ./store\nregistries:\n  local:\n    type: hosted\ndefaultRegistry: local\n",
    )
    .unwrap();
    let config = crate::Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    let state = state_and_router(config, AuthState::in_memory(), None).unwrap().0;
    let pinned = state.routing();
    let (swapped, wait_for_swap) = tokio::sync::oneshot::channel::<()>();
    let worker_state = state.clone();
    let spawned = PINNED.sync_scope(Arc::clone(&pinned), || {
        tokio::spawn(carry_pinned_routing(async move {
            wait_for_swap.await.unwrap();
            worker_state.routing()
        }))
    });

    state.test_replace_routing();
    swapped.send(()).unwrap();
    let seen = spawned.await.unwrap();
    assert!(Arc::ptr_eq(&seen, &pinned));
    assert!(!Arc::ptr_eq(&state.routing(), &pinned));
}
