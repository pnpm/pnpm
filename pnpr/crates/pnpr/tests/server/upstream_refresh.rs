use super::{
    Body, Duration, Request, ServiceExt, StatusCode, TempDir, body_bytes, config_for,
    foo_packument, json, public_cache_blob, router,
};

#[tokio::test]
async fn cached_tarball_is_evicted_when_refresh_removes_its_version() {
    let mut upstream = mockito::Server::new_async().await;
    let initial = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(foo_packument(&upstream.url()).to_string())
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().maxage = Some(Duration::ZERO);
    let app = router(config);
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cache = public_cache_blob(tmp.path(), "foo", "foo-1.0.0.tgz");
    tokio::fs::create_dir_all(cache.parent().unwrap()).await.unwrap();
    tokio::fs::write(&cache, b"formerly verified bytes").await.unwrap();
    initial.assert_async().await;
    initial.remove_async().await;
    let removed = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({"name": "foo", "versions": {}, "dist-tags": {}}).to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body("removed bytes")
        .expect(0)
        .create_async()
        .await;
    let response = app
        .oneshot(
            Request::get("/npm/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let _ = body_bytes(response.into_body()).await;
    assert!(!cache.exists());
    removed.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn backfill_runs_in_parallel_without_blocking_responses_and_preserves_pins() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"example archive";
    let mut doc = foo_packument(&upstream.url());
    for version in ["1.0.0", "2.0.0"] {
        doc["versions"][version] = json!({"name": "foo", "version": version, "dist": {
            "tarball": format!("{}/foo/-/foo-{version}.tgz", upstream.url()),
            "shasum": super::sha1_hex_of(bytes),
        }});
    }
    let metadata = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(doc.to_string())
        .expect(2)
        .create_async()
        .await;
    let (started, mut downloads) = tokio::sync::mpsc::unbounded_channel();
    let mut releases = Vec::new();
    let mut mocks = Vec::new();
    for version in ["1.0.0", "2.0.0"] {
        let (release, ready) = std::sync::mpsc::channel();
        releases.push(release);
        let ready = std::sync::Mutex::new(ready);
        let started = started.clone();
        mocks.push(
            upstream
                .mock("GET", format!("/foo/-/foo-{version}.tgz").as_str())
                .with_status(200)
                .with_chunked_body(move |writer| {
                    started.send(()).unwrap();
                    ready
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                    writer.write_all(bytes)
                })
                .expect(1)
                .create_async()
                .await,
        );
    }
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().maxage = Some(Duration::ZERO);
    let app = router(config);
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        app.clone()
            .oneshot(
                Request::get("/npm/foo")
                    .body(Body::empty())
                    .unwrap(),
            ),
    )
    .await
    .expect("metadata response must not wait for tarballs")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let doc: serde_json::Value =
        serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(doc["versions"]["1.0.0"]["dist"].get("integrity").is_none());
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(2), downloads.recv()).await
            .expect("both downloads must start concurrently")
            .unwrap();
    }
    for release in releases {
        release.send(()).unwrap();
    }
    super::integrity_backfill::wait_for_pins(tmp.path(), 2).await;
    let response = app
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let doc: serde_json::Value =
        serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    for version in ["1.0.0", "2.0.0"] {
        assert_eq!(doc["versions"][version]["dist"]["integrity"], super::sha512_integrity(bytes));
    }
    metadata.assert_async().await;
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn backfill_reuses_a_cached_tarball() {
    backfill_cached_tarball(b"cached example archive", 0).await;
}

#[tokio::test]
async fn backfill_replaces_a_cached_tarball_that_fails_its_shasum() {
    backfill_cached_tarball(b"corrupted archive", 1).await;
}

async fn backfill_cached_tarball(cached_bytes: &[u8], downloads: usize) {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"cached example archive";
    let mut doc = foo_packument(&upstream.url());
    doc["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    doc["versions"]["1.0.0"]["dist"]["shasum"] = json!(super::sha1_hex_of(bytes));
    let unpinned = doc.to_string();
    doc["versions"]["1.0.0"]["dist"]["integrity"] = json!(super::sha512_integrity(bytes));
    let initial = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(doc.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(downloads)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().maxage = Some(Duration::ZERO);
    let app = router(config.clone());
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    initial.remove_async().await;
    let cache = public_cache_blob(tmp.path(), "foo", "foo-1.0.0.tgz");
    tokio::fs::create_dir_all(cache.parent().unwrap()).await.unwrap();
    tokio::fs::write(cache, cached_bytes).await.unwrap();
    tokio::fs::write(super::public_cache_pkg(tmp.path(), "foo").join("document.json"), &unpinned)
        .await
        .unwrap();
    let metadata = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(unpinned)
        .expect(1)
        .create_async()
        .await;
    let response = router(config)
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let doc = super::integrity_backfill::wait_for_pins(tmp.path(), 1).await;
    assert_eq!(doc["versions"]["1.0.0"]["dist"]["integrity"], super::sha512_integrity(bytes));
    metadata.assert_async().await;
    tarball.assert_async().await;
}
