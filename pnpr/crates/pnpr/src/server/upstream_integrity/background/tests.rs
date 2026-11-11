use super::{MAX_ACTIVE_PACKUMENTS, schedule, wait};
use crate::server::{AppState, routing::state_and_router};
use pnpr_auth::AuthState;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
use std::time::Duration;

fn state(dir: &std::path::Path, upstream: &str) -> AppState {
    let path = dir.join("pnpr.yaml");
    std::fs::write(&path, format!("storage: ./store\nregistries:\n  mirror:\n    type: upstream\n    url: {upstream}\n    public: true\ndefaultRegistry: mirror\n")).unwrap();
    let config = crate::Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    state_and_router(config, AuthState::in_memory(), None).unwrap().0
}

#[tokio::test]
async fn queued_packages_complete_after_capacity_returns() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = state(tmp.path(), &server.url());
    let upstream = &state.inner.proxy.upstreams["mirror"];
    let held = state.inner.proxy.integrity_backfill.slots
        .acquire_many(MAX_ACTIVE_PACKUMENTS as u32)
        .await
        .unwrap();
    let mut packages = Vec::new();
    let mut mocks = Vec::new();
    for index in 0..=MAX_ACTIVE_PACKUMENTS {
        let name =
            CanonicalPackageName::parse(&format!("example-{index}"), Ecosystem::Npm).unwrap();
        let filename = name.tarball_name_for_version("1.0.0");
        let route = format!("/{}/-/{filename}", name.as_str());
        mocks.push(
            server
                .mock("GET", route.as_str())
                .with_status(200)
                .with_body("archive")
                .expect(1)
                .create_async()
                .await,
        );
        let doc = json!({"name":name.as_str(), "versions":{"1.0.0":{"dist":{"tarball":format!("{}{route}", server.url())}}}});
        state.inner.storage
            .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
            .await
            .unwrap();
        schedule(&state, "test", upstream, &name);
        packages.push(name);
    }
    assert_eq!(
        state.inner.proxy.integrity_backfill.jobs
            .lock()
            .unwrap()
            .len(),
        MAX_ACTIVE_PACKUMENTS + 1,
    );
    drop(held);
    for name in packages {
        tokio::time::timeout(Duration::from_secs(10), wait(&state, "test", &name))
            .await
            .unwrap();
        let bytes = state.inner.storage
            .read_upstream_document_any("test", &name)
            .await
            .unwrap()
            .unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            doc["versions"]["1.0.0"]["dist"]["integrity"],
            ssri::IntegrityOpts::new()
                .algorithm(ssri::Algorithm::Sha512)
                .chain(b"archive")
                .result()
                .to_string(),
        );
    }
    for mock in mocks {
        mock.assert_async().await;
    }
    assert!(
        state.inner.proxy.integrity_backfill.jobs
            .lock()
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn a_refresh_during_backfill_retries_the_new_document() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = state(tmp.path(), &server.url());
    let upstream = &state.inner.proxy.upstreams["mirror"];
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let url = server.url();
    let document = |file: &str| {
        serde_json::to_vec(&json!({"versions":{"1.0.0":{"dist":{
            "tarball":format!("{url}/example/-/{file}.tgz")
        }}}}))
        .unwrap()
    };
    let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let (release, ready) = std::sync::mpsc::channel();
    let ready = std::sync::Mutex::new(ready);
    let old = server
        .mock("GET", "/example/-/old.tgz")
        .with_status(200)
        .with_chunked_body(move |writer| {
            started.send(()).unwrap();
            ready
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            writer.write_all(b"old")
        })
        .expect(1)
        .create_async()
        .await;
    let new = server
        .mock("GET", "/example/-/new.tgz")
        .with_status(200)
        .with_body("new")
        .expect(1)
        .create_async()
        .await;
    state.inner.storage
        .write_upstream_document("test", &name, &document("old"))
        .await
        .unwrap();
    schedule(&state, "test", upstream, &name);
    tokio::time::timeout(Duration::from_secs(10), observed.recv()).await.unwrap().unwrap();
    super::super::refresh_cached_packument(&state, "test", &name, document("new"))
        .await
        .unwrap();
    schedule(&state, "test", upstream, &name);
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), wait(&state, "test", &name))
        .await
        .unwrap();
    let bytes = state.inner.storage
        .read_upstream_document_any("test", &name)
        .await
        .unwrap()
        .unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["integrity"],
        ssri::IntegrityOpts::new()
            .algorithm(ssri::Algorithm::Sha512)
            .chain(b"new")
            .result()
            .to_string(),
    );
    assert!(
        state.inner.storage
            .open_upstream_blob("test", &name, "old.tgz")
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        state.inner.storage
            .open_upstream_blob("test", &name, "new.tgz")
            .await
            .unwrap()
            .is_some(),
    );
    old.assert_async().await;
    new.assert_async().await;
}
