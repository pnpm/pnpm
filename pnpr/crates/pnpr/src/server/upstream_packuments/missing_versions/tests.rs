use super::{AppState, CanonicalPackageName, PackumentRefetches};
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::sync::Arc;
use tempfile::TempDir;

fn package(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm).unwrap()
}

#[test]
fn a_package_is_claimed_once_per_interval() {
    let refetches = PackumentRefetches::default();
    assert!(refetches.claim("npmjs", &package("example-widget")));
    assert!(!refetches.claim("npmjs", &package("example-widget")));
    assert!(refetches.claim("npmjs", &package("example-gadget")));
    assert!(refetches.claim("mirror", &package("example-widget")));
}

/// Another request can refetch the packument between the caller's read and
/// its reload. The reload then returns that copy rather than nothing.
#[tokio::test]
async fn a_packument_refetched_since_the_read_is_returned_without_a_fetch() {
    let mut upstream = mockito::Server::new_async().await;
    let fetch = upstream
        .mock("GET", "/example-widget")
        .expect(0)
        .create_async()
        .await;
    let storage = TempDir::new().unwrap();
    let mut config = Config::proxy("127.0.0.1:0".parse().unwrap(), storage.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().url = upstream.url();
    config.http.public_url = "http://example.test".to_string();
    config.routing.route_policy.allowed_private_networks =
        vec![pnpr_config::IpNetwork::parse("127.0.0.0/8").unwrap()];
    let state = AppState {
        inner: Arc::new(
            crate::server::state::AppInner::new(config, pnpr_auth::AuthState::in_memory(), None)
                .unwrap(),
        ),
    };
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let name = package("example-widget");
    let refetched = br#"{"name":"example-widget","versions":{}}"#;
    state.inner.storage.write_upstream_document(&namespace, &name, refetched).await.unwrap();

    let reloaded = super::reload_upstream_packument_lacking_version(
        &state,
        &namespace,
        &state.routing().upstream_clients["npmjs"],
        &name,
    )
    .await
    .unwrap();

    assert_eq!(reloaded.as_deref(), Some(refetched.as_slice()));
    fetch.assert_async().await;
}
