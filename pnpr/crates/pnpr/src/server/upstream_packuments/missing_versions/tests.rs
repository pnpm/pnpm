use super::{
    AppState,
    CAPACITY,
    CanonicalPackageName,
    PackumentRefetches,
};
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::{
    path::Path,
    sync::Arc,
    time::{
        Duration,
        SystemTime,
    },
};
use tempfile::TempDir;

fn package(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm).unwrap()
}

#[test]
fn a_package_is_claimed_once_per_interval() {
    let refetches = PackumentRefetches::default();
    assert!(refetches.claim_forced("npmjs".to_owned(), &package("example-widget")));
    assert!(!refetches.claim_forced("npmjs".to_owned(), &package("example-widget")));
    assert!(refetches.claim_forced("npmjs".to_owned(), &package("example-gadget")));
    assert!(refetches.claim_forced("mirror".to_owned(), &package("example-widget")));
}

#[test]
fn a_new_package_is_claimed_when_recent_attempts_fill_the_table() {
    let refetches = PackumentRefetches::default();
    for index in 0..CAPACITY {
        assert!(refetches.claim_forced(
            "npmjs".to_owned(),
            &package(&format!("example-widget-{index}"))
        ));
    }
    assert!(refetches.claim_forced("npmjs".to_owned(), &package("example-gadget")));
    assert!(
        refetches.forced_attempts
            .lock()
            .unwrap()
            .len()
            <= CAPACITY,
    );
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
    let state = proxy_state(&upstream.url(), storage.path());
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let name = package("example-widget");
    let refetched = br#"{"name":"example-widget","versions":{}}"#;
    state.inner.storage.write_upstream_document(&namespace, &name, refetched).await.unwrap();

    let reloaded = reload(&state, &name).await;

    assert_eq!(reloaded.as_deref(), Some(refetched.as_slice()));
    fetch.assert_async().await;
}

/// The claim is taken once a refetch starts, so a request arriving during it
/// must wait for its result rather than give up.
#[tokio::test]
async fn a_request_during_a_refetch_gets_its_result() {
    use axum::{
        Json,
        Router,
        routing::get,
    };

    let release = Arc::new(tokio::sync::Notify::new());
    let held = Arc::clone(&release);
    let (fetched_tx, mut fetched) = tokio::sync::mpsc::unbounded_channel();
    let listing = serde_json::json!({"name": "example-widget", "versions": {"2.0.0": {}}});
    let served = listing.clone();
    let origin = Router::new()
        .route(
            "/example-widget",
            get(move || async move {
                fetched_tx.send(()).unwrap();
                held.notified().await;
                Json(served)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = tokio::spawn(async move { axum::serve(listener, origin).await.unwrap() });
    let storage = TempDir::new().unwrap();
    let state = proxy_state(&format!("http://{address}"), storage.path());
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let name = package("example-widget");
    let cached = br#"{"name":"example-widget","versions":{}}"#;
    state.inner.storage.write_upstream_document(&namespace, &name, cached).await.unwrap();
    date_back(storage.path(), Duration::from_mins(2));

    let first = tokio::spawn({
        let state = state.clone();
        let name = name.clone();
        async move { reload(&state, &name).await }
    });
    fetched.recv().await.unwrap();
    let second = tokio::spawn({
        let state = state.clone();
        let name = name.clone();
        async move { reload(&state, &name).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    release.notify_waiters();

    for reloaded in [first.await.unwrap(), second.await.unwrap()] {
        let reloaded: serde_json::Value = serde_json::from_slice(&reloaded.unwrap()).unwrap();
        assert!(reloaded["versions"].get("2.0.0").is_some());
    }
    assert!(fetched.try_recv().is_err(), "one upstream fetch serves both requests");
    origin.abort();
}

/// A packument can expire between the caller's read and its reload. Only a
/// failed fetch, not the clock, says the upstream was already asked.
#[tokio::test]
async fn a_packument_that_expired_since_the_read_is_refetched() {
    let mut upstream = mockito::Server::new_async().await;
    let listing = r#"{"name":"example-widget","versions":{"2.0.0":{}}}"#;
    let fetch = upstream
        .mock("GET", "/example-widget")
        .with_header("content-type", "application/json")
        .with_body(listing)
        .expect(1)
        .create_async()
        .await;
    let storage = TempDir::new().unwrap();
    let state = proxy_state(&upstream.url(), storage.path());
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let name = package("example-widget");
    let cached = br#"{"name":"example-widget","versions":{}}"#;
    state.inner.storage.write_upstream_document(&namespace, &name, cached).await.unwrap();
    date_back(storage.path(), Duration::from_hours(25));

    let reader = state.routing().upstream_clients["npmjs"].clone().cached_only("reader".to_owned());
    let denied =
        super::reload_upstream_packument_lacking_version(&state, &namespace, &reader, &name).await;
    assert!(matches!(denied, Err(pnpr_error::RegistryError::Forbidden { .. })));

    let reloaded: serde_json::Value =
        serde_json::from_slice(&reload(&state, &name).await.unwrap()).unwrap();

    assert!(reloaded["versions"].get("2.0.0").is_some());
    fetch.assert_async().await;
}

fn proxy_state(upstream: &str, storage: &Path) -> AppState {
    let mut config = Config::proxy("127.0.0.1:0".parse().unwrap(), storage.to_path_buf());
    let npmjs = config.routing.upstreams.get_mut("npmjs").unwrap();
    npmjs.url = upstream.to_string();
    npmjs.packument_ttl = Some(Duration::from_hours(24));
    config.http.public_url = Some("http://example.test".to_string());
    config.routing.route_policy.allowed_private_networks =
        vec![pnpr_config::IpNetwork::parse("127.0.0.0/8").unwrap()];
    AppState {
        inner: Arc::new(
            crate::server::state::AppInner::new(config, pnpr_auth::AuthState::in_memory(), None)
                .unwrap(),
        ),
    }
}

async fn reload(state: &AppState, name: &CanonicalPackageName) -> Option<Vec<u8>> {
    let namespace = crate::server::upstream_cache_namespace(state, "npmjs");
    super::reload_upstream_packument_lacking_version(
        state,
        &namespace,
        &state.routing().upstream_clients["npmjs"],
        name,
    )
    .await
    .unwrap()
}

/// Date every file under `dir` back by `age`, so a cached packument reads as
/// fetched that long ago.
fn date_back(dir: &Path, age: Duration) {
    let fetched = SystemTime::now() - age;
    let files = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file());
    for entry in files {
        std::fs::File::options()
            .write(true)
            .open(entry.path())
            .unwrap()
            .set_modified(fetched)
            .unwrap();
    }
}
