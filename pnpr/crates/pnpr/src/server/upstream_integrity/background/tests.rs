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

#[tokio::test]
async fn failed_pins_are_not_retried_by_download_requests() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = state(tmp.path(), &server.url());
    let upstream = &state.inner.proxy.upstreams["mirror"];
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let filename = "example-1.0.0.tgz";
    let archive = server
        .mock("GET", "/example/-/example-1.0.0.tgz")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let doc = json!({"versions":{"1.0.0":{"dist":{"tarball":format!("{}/example/-/{filename}", server.url())}}}});
    state.inner.storage
        .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
        .await
        .unwrap();
    schedule(&state, "test", upstream, &name);
    wait(&state, "test", &name).await;
    for _ in 0..2 {
        super::wait_for_pin(&state, "test", upstream, &name, filename).await.unwrap();
    }
    assert!(
        state.inner.proxy.integrity_backfill.jobs
            .lock()
            .unwrap()
            .is_empty(),
    );
    archive.assert_async().await;
}

#[tokio::test]
async fn pin_waits_are_bounded_and_skip_unusable_metadata() {
    let server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let state = state(tmp.path(), &server.url());
    let mut config = state.inner.config.routing.upstreams["mirror"].clone();
    config.requests.timeout = Duration::from_millis(20);
    let upstream = super::Upstream::new("mirror", &config);
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let filename = "example-1.0.0.tgz";
    let mut doc = json!({"versions":{"1.0.0":{"dist":{"tarball":format!("{}/example/-/{filename}", server.url())}}}});
    state.inner.storage
        .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
        .await
        .unwrap();
    state.inner.proxy.integrity_backfill.jobs
        .lock()
        .unwrap()
        .insert(
            ("test".to_string(), name.as_str().to_string()),
            super::PendingJob { changed: false, completion: tokio::sync::watch::channel(()).0 },
        );
    tokio::time::timeout(
        Duration::from_secs(1),
        super::wait_for_pin(&state, "test", &upstream, &name, filename),
    )
    .await
    .unwrap()
    .unwrap();
    doc["versions"]["1.0.0"]["dist"]["integrity"] = json!("invalid");
    state.inner.storage
        .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
        .await
        .unwrap();
    config.requests.timeout = Duration::from_secs(10);
    let upstream = super::Upstream::new("mirror", &config);
    tokio::time::timeout(
        Duration::from_secs(1),
        super::wait_for_pin(&state, "test", &upstream, &name, filename),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn a_download_resumes_cached_work_after_restart() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let app = state(tmp.path(), &server.url());
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let filename = "example-1.0.0.tgz";
    let archive = server
        .mock("GET", "/example/-/example-1.0.0.tgz")
        .with_status(200)
        .with_body("archive")
        .expect(1)
        .create_async()
        .await;
    let doc = json!({"versions":{"1.0.0":{"dist":{"tarball":format!("{}/example/-/{filename}", server.url())}}}});
    app.inner.storage
        .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
        .await
        .unwrap();
    drop(app);
    let app = state(tmp.path(), &server.url());
    let upstream = &app.inner.proxy.upstreams["mirror"];
    super::wait_for_pin(&app, "test", upstream, &name, filename).await.unwrap();
    let bytes = app.inner.storage
        .read_upstream_document_any("test", &name)
        .await
        .unwrap()
        .unwrap();
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(doc["versions"]["1.0.0"]["dist"]["integrity"].as_str().is_some());
    archive.assert_async().await;
}

#[tokio::test]
async fn waiting_applies_the_cap_after_vulnerability_screening() {
    let mut server = mockito::Server::new_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let mut app = state(tmp.path(), &server.url());
    let advisories = tmp.path().join("advisories");
    std::fs::create_dir(&advisories).unwrap();
    let advisory = advisories.join("advisory.json");
    std::fs::write(&advisory, serde_json::to_vec(&json!({
        "id":"GHSA-example", "affected":[{"package":{"ecosystem":"npm","name":"example"},"versions":["1.0.0"]}]
    })).unwrap()).unwrap();
    std::sync::Arc::get_mut(&mut app.inner).unwrap().osv_index =
        Some(std::sync::Arc::new(pnpr_osv::OsvIndex::load_from_path(&advisories).unwrap()));
    let name = CanonicalPackageName::parse("example", Ecosystem::Npm).unwrap();
    let mut doc = json!({"versions":{}});
    for index in 0..65 {
        doc["versions"][format!("1.0.{index}")] = json!({"dist":{"tarball":format!("{}/example/-/example-1.0.{index}.tgz",server.url())}});
    }
    let last = doc["versions"]
        .as_object()
        .unwrap()
        .keys()
        .next_back()
        .unwrap()
        .clone();
    let filename = format!("example-{last}.tgz");
    app.inner.storage
        .write_upstream_document("test", &name, &serde_json::to_vec(&doc).unwrap())
        .await
        .unwrap();
    let archive = server
        .mock("GET", mockito::Matcher::Regex("/example/-/example-.*".to_string()))
        .with_status(200)
        .with_body("archive")
        .expect(64)
        .create_async()
        .await;
    let held = app.inner.proxy.integrity_backfill.slots
        .acquire_many(MAX_ACTIVE_PACKUMENTS as u32)
        .await
        .unwrap();
    let upstream = &app.inner.proxy.upstreams["mirror"];
    schedule(&app, "test", upstream, &name);
    let waiter_state = app.clone();
    let waiter_name = name.clone();
    let mut waiter = tokio::spawn(async move {
        let upstream = &waiter_state.inner.proxy.upstreams["mirror"];
        super::wait_for_pin(&waiter_state, "test", upstream, &waiter_name, &filename)
            .await
            .unwrap();
    });
    assert!(tokio::time::timeout(Duration::from_millis(100), &mut waiter).await.is_err());
    drop(held);
    tokio::time::timeout(Duration::from_secs(10), waiter).await.unwrap().unwrap();
    let bytes = app.inner.storage
        .read_upstream_document_any("test", &name)
        .await
        .unwrap()
        .unwrap();
    let pinned: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(pinned["versions"][&last]["dist"]["integrity"].as_str().is_some());
    archive.assert_async().await;
}
