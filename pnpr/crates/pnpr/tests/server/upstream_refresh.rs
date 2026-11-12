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
    tokio::fs::write(&cache, cached_bytes).await.unwrap();
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
    tokio::time::timeout(Duration::from_secs(10), async {
        while tokio::fs::read(&cache).await.unwrap().as_slice() != bytes {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the cache contains the verified bytes");
    metadata.assert_async().await;
    tarball.assert_async().await;
}

#[tokio::test]
async fn an_inflight_download_cannot_restore_an_evicted_blob() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"old archive";
    let mut doc = foo_packument(&upstream.url());
    doc["versions"]["1.0.0"]["dist"]["integrity"] =
        json!(ssri::Integrity::from(bytes.as_slice()).to_string());
    let initial = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(doc.to_string())
        .create_async()
        .await;
    let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let (release, ready) = std::sync::mpsc::channel();
    let ready = std::sync::Mutex::new(ready);
    let archive = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_chunked_body(move |writer| {
            writer.write_all(bytes)?;
            writer.flush()?;
            started.send(()).unwrap();
            ready
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            Ok(())
        })
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
            Request::get("/npm/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let download = tokio::spawn(body_bytes(response.into_body()));
    tokio::time::timeout(Duration::from_secs(10), observed.recv()).await
        .expect("the archive request starts")
        .unwrap();
    initial.remove_async().await;
    let removed = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({"name":"foo", "versions":{}}).to_string())
        .create_async()
        .await;
    let response = app
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    release.send(()).unwrap();
    assert_eq!(download.await.unwrap(), bytes);
    assert!(!public_cache_blob(tmp.path(), "foo", "foo-1.0.0.tgz").exists());
    archive.assert_async().await;
    removed.assert_async().await;
}

#[tokio::test]
async fn version_downloads_refresh_metadata_before_serving_cached_bytes() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"archive";
    let mut doc = foo_packument(&upstream.url());
    doc["versions"]["1.0.0"]["dist"]["integrity"] =
        json!(ssri::Integrity::from(bytes.as_slice()).to_string());
    let initial = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(doc.to_string())
        .create_async()
        .await;
    let archive = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
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
            Request::get("/npm/foo@1.0.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    initial.remove_async().await;
    let stale_manifest = upstream
        .mock("GET", "/foo/1.0.0")
        .with_status(200)
        .with_body(doc["versions"]["1.0.0"].to_string())
        .expect(0)
        .create_async()
        .await;
    let removed = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({"name":"foo", "versions":{}}).to_string())
        .create_async()
        .await;
    let response = app
        .oneshot(
            Request::get("/npm/foo@1.0.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    stale_manifest.assert_async().await;
    archive.assert_async().await;
    removed.assert_async().await;
}

#[tokio::test]
async fn cold_downloads_wait_for_missing_integrity() {
    for (route, integrity) in [
        ("/npm/foo/-/foo-1.0.0.tgz", None),
        ("/npm/foo@1.0.0", None),
        ("/npm/foo/-/foo-1.0.0.tgz", Some("")),
        ("/npm/foo@1.0.0", Some("")),
    ] {
        let mut upstream = mockito::Server::new_async().await;
        let bytes = b"archive requiring a pin";
        let mut doc = foo_packument(&upstream.url());
        doc["versions"]["1.0.0"]["dist"]
            .as_object_mut()
            .unwrap()
            .remove("integrity");
        doc["versions"]["1.0.0"]["dist"]
            .as_object_mut()
            .unwrap()
            .remove("shasum");
        if let Some(integrity) = integrity {
            doc["versions"]["1.0.0"]["dist"]["integrity"] = json!(integrity);
        }
        let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
        let (release, ready) = std::sync::mpsc::channel();
        let ready = std::sync::Mutex::new(ready);
        let metadata = upstream
            .mock("GET", "/foo")
            .with_status(200)
            .with_body(doc.to_string())
            .expect(1)
            .create_async()
            .await;
        let archive = upstream
            .mock("GET", "/foo/-/foo-1.0.0.tgz")
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
            .await;
        let tmp = TempDir::new().unwrap();
        let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
        let request = tokio::spawn(
            app.oneshot(
                Request::get(route)
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        tokio::time::timeout(Duration::from_secs(10), observed.recv()).await
            .expect("the archive request starts")
            .unwrap();
        assert!(!request.is_finished());
        release.send(()).unwrap();
        let response = request.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_eq!(body_bytes(response.into_body()).await, bytes, "{route}");
        metadata.assert_async().await;
        archive.assert_async().await;
    }
}

#[tokio::test]
async fn the_first_packument_evicts_orphaned_blobs() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"current archive";
    let mut doc = foo_packument(&upstream.url());
    doc["versions"]["1.0.0"]["dist"]["integrity"] = json!(super::sha512_integrity(bytes));
    let metadata = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(doc.to_string())
        .expect(1)
        .create_async()
        .await;
    let archive = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let namespace = pnpr::cache_maintenance::active_cache_namespaces(&config)
        .into_iter()
        .find(|namespace| namespace.starts_with("~public/"))
        .unwrap();
    tokio::fs::create_dir_all(
        tmp.path()
            .join(".pnpr-cache")
            .join(namespace),
    )
    .await
    .unwrap();
    let cache = public_cache_blob(tmp.path(), "foo", "foo-1.0.0.tgz");
    tokio::fs::create_dir_all(cache.parent().unwrap()).await.unwrap();
    tokio::fs::write(&cache, b"orphaned archive").await.unwrap();
    let app = router(config);
    let response = app
        .oneshot(
            Request::get("/npm/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    assert_eq!(tokio::fs::read(&cache).await.unwrap(), bytes);
    metadata.assert_async().await;
    archive.assert_async().await;
}

#[tokio::test]
async fn removing_a_package_rejects_an_inflight_manifest_download() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"manifest archive";
    let manifest = upstream
        .mock("GET", "/foo/1.0.0")
        .with_status(200)
        .with_body(
            json!({"name":"foo","version":"1.0.0","dist":{
                "tarball":format!("{}/foo/-/foo-1.0.0.tgz",upstream.url()),
                "integrity":super::sha512_integrity(bytes)
            }})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let removed = upstream
        .mock("GET", "/foo")
        .with_status(404)
        .expect(2)
        .create_async()
        .await;
    let (started, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let (release, ready) = std::sync::mpsc::channel();
    let ready = std::sync::Mutex::new(ready);
    let archive = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_chunked_body(move |writer| {
            writer.write_all(bytes)?;
            writer.flush()?;
            started.send(()).unwrap();
            ready
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            Ok(())
        })
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo@1.0.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let download = tokio::spawn(body_bytes(response.into_body()));
    tokio::time::timeout(Duration::from_secs(10), observed.recv()).await.unwrap().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    release.send(()).unwrap();
    assert_eq!(download.await.unwrap(), bytes);
    assert!(!public_cache_blob(tmp.path(), "foo", "foo-1.0.0.tgz").exists());
    manifest.assert_async().await;
    archive.assert_async().await;
    manifest.remove_async().await;
    archive.remove_async().await;
    let response = app
        .oneshot(
            Request::get("/npm/foo@1.0.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    removed.assert_async().await;
}
