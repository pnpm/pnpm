use super::{
    Body, Duration, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, config_for,
    foo_packument, json, public_cache_blobs, public_cache_pkg, router, sha1_hex_of,
    sha512_integrity, tarball_cache_entries, to_bytes,
};

#[tokio::test]
async fn missing_integrity_is_computed_cached_and_returned_in_the_packument() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"computed-integrity-tarball";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("shasum");
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

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
    assert_eq!(
        tarball_cache_entries(&public_cache_blobs(&storage, "foo")),
        vec!["1.0.0.tgz".to_string()],
    );
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// npm keeps publishing a version's metadata after withdrawing its tarball,
/// and the benchmark fixture graph hits several such versions.
#[tokio::test]
async fn version_with_unfetchable_tarball_does_not_break_the_packument() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"the-one-tarball-that-exists";
    let mut packument = foo_packument(&upstream.url());
    for version in ["0.3.0", "0.1.0"] {
        packument["versions"][version] = json!({
            "name": "foo",
            "version": version,
            "dist": {
                "tarball": format!("{}/foo/-/foo-{version}.tgz", upstream.url()),
                "shasum": "0000000000000000000000000000000000000000",
            }
        });
    }
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("shasum");
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    // Only 1.0.0 is downloadable; the withdrawn versions 404, as npm answers.
    let mut withdrawn = Vec::new();
    for version in ["0.3.0", "0.1.0"] {
        let mock = upstream
            .mock("GET", format!("/foo/-/foo-{version}.tgz").as_str())
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        withdrawn.push(mock);
    }
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

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
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
    for mock in withdrawn {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn version_with_unfetchable_tarball_keeps_its_declared_shasum() {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("shasum");
    packument["versions"]["0.1.0"] = json!({
        "name": "foo",
        "version": "0.1.0",
        "dist": {
            "tarball": format!("{}/foo/-/foo-0.1.0.tgz", upstream.url()),
            "shasum": "0000000000000000000000000000000000000000",
        }
    });
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let withdrawn = upstream
        .mock("GET", "/foo/-/foo-0.1.0.tgz")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        resolved["versions"]["0.1.0"]["dist"]["shasum"],
        "0000000000000000000000000000000000000000",
    );
    assert!(
        resolved["versions"]["0.1.0"]["dist"].get("integrity").is_none(),
        "an unfetchable tarball has no bytes to hash, so no integrity may be invented",
    );
    wait_for_mock(&withdrawn).await;
    withdrawn.assert_async().await;
}

/// The downloads can outlast a short `packument_ttl`. A cached packument that went
/// stale meanwhile still receives the pins, so they are not lost.
#[tokio::test]
async fn pins_land_even_when_the_cached_packument_went_stale_meanwhile() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"stale-while-pinning";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex_of(bytes));
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").packument_ttl =
        Some(Duration::ZERO);
    let response = router(config)
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));
}

#[tokio::test]
async fn a_refresh_that_cannot_reconcile_the_cache_fails_closed() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"cache-write-fails";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex_of(bytes));
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let mut config = config_for(&upstream.url(), storage.clone());
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").packument_ttl =
        Some(Duration::ZERO);
    let app = router(config);
    let resolve = || {
        app.clone()
            .oneshot(
                Request::get("/npm/foo")
                    .header("accept", "application/vnd.npm.install-v1+json")
                    .body(Body::empty())
                    .unwrap(),
            )
    };

    let response = resolve().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    // A directory where the packument file belongs makes the next cache write fail.
    let document = public_cache_pkg(&storage, "foo").join("document.json");
    std::fs::remove_file(&document).unwrap();
    std::fs::create_dir_all(document.join("blocker")).unwrap();

    let response = resolve().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// Pinning runs when pnpr fetches a packument, not on every read of the cached
/// copy, so a version that could not be pinned costs one upstream request per
/// refresh rather than one per read.
#[tokio::test]
async fn cached_packument_reads_do_not_refetch_unpinnable_tarballs() {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/npm/foo")
                    .header("accept", "application/vnd.npm.install-v1+json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let resolved: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert!(resolved["versions"]["1.0.0"]["dist"].get("integrity").is_none());
    }
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// One version's dist that pnpr cannot use must not make the rest of the
/// package unresolvable. The tarball route still refuses that version.
#[tokio::test]
async fn version_with_malformed_dist_does_not_break_the_packument() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"the-pinnable-tarball";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex_of(bytes));
    packument["versions"]["0.1.0"] = json!({
        "name": "foo",
        "version": "0.1.0",
        "dist": {
            "tarball": format!("{}/foo/-/foo-0.1.0.tgz", upstream.url()),
            "integrity": "not-a-valid-sri",
        }
    });
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let malformed_tarball = upstream
        .mock("GET", "/foo/-/foo-0.1.0.tgz")
        .with_status(200)
        .with_body("unverified")
        .expect(0)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    let response = app
        .oneshot(
            Request::get("/npm/foo/-/foo-0.1.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    malformed_tarball.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// A computed integrity is only worth anything if it can be stored with the
/// cached packument, so pnpr makes no attempt for an upstream that caches
/// nothing.
#[tokio::test]
async fn cache_disabled_upstream_serves_the_packument_it_declared() {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("shasum");
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body("untrusted")
        .expect(0)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").cache = false;

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
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(resolved["versions"]["1.0.0"]["dist"].get("integrity").is_none());

    let response = app
        .oneshot(
            Request::get("/npm/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// The candidate set is the upstream's to choose, so one client request must
/// not fan out into a tarball fetch per unpinned version it declares.
#[tokio::test]
async fn the_pinned_version_count_is_capped_per_packument() {
    const PINNED: usize = 64;
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"capped-tarball";
    let mut packument = foo_packument(&upstream.url());
    for index in 0..=PINNED {
        let version = format!("0.0.{index}");
        packument["versions"][&version] = json!({
            "name": "foo",
            "version": version,
            "dist": {
                "tarball": format!("{}/foo/-/foo-{version}.tgz", upstream.url()),
                "shasum": sha1_hex_of(bytes),
            }
        });
    }
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    for index in 0..PINNED {
        upstream
            .mock("GET", format!("/foo/-/foo-0.0.{index}.tgz").as_str())
            .with_status(200)
            .with_body(bytes)
            .expect(1)
            .create_async()
            .await;
    }
    // The one version past the cap must never be fetched.
    let beyond_cap = upstream
        .mock("GET", format!("/foo/-/foo-0.0.{PINNED}.tgz").as_str())
        .with_status(200)
        .with_body(bytes)
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), PINNED + 1).await;
    for index in 0..PINNED {
        let version = format!("0.0.{index}");
        assert_eq!(
            resolved["versions"][&version]["dist"]["integrity"],
            sha512_integrity(bytes),
            "version {version} is within the cap and must be pinned",
        );
    }
    let capped = format!("0.0.{PINNED}");
    assert!(
        resolved["versions"][&capped]["dist"].get("integrity").is_none(),
        "the version past the cap keeps the integrity the upstream declared",
    );
    assert_eq!(resolved["versions"][&capped]["dist"]["shasum"], sha1_hex_of(bytes));
    packument_mock.assert_async().await;
    beyond_cap.assert_async().await;
}

/// An SRI that parses to no hashes pins nothing, which is the shape a client
/// reads as a missing integrity, so pnpr computes one instead of treating the
/// version as malformed.
#[tokio::test]
async fn an_integrity_string_that_pins_nothing_is_computed() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"empty-integrity-tarball";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]["integrity"] = json!("");
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("shasum");
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(
            Request::get("/npm/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));
}

/// A pre-2017 npm publish carries only the legacy hex `dist.shasum`. pnpr
/// computes a modern SRI and stores it with the cached packument and tarball.
#[tokio::test]
async fn shasum_only_tarball_gets_a_computed_sha512_integrity() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"legacy-shasum-tarball";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex_of(bytes));
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
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
    let _ = body_bytes(response.into_body()).await;
    let resolved = wait_for_pins(tmp.path(), 1).await;
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

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
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

/// Bytes that contradict the upstream's own `dist.shasum` are never cached and
/// never served, but they must not cost the package its other versions.
#[tokio::test]
async fn shasum_only_tarball_with_mismatched_bytes_is_not_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"tarball-does-not-match-its-shasum";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    packument["versions"]["1.0.0"]["dist"]["shasum"] =
        json!(sha1_hex_of(b"different tarball bytes"));
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let (release, ready) = std::sync::mpsc::channel();
    let ready = std::sync::Mutex::new(Some(ready));
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_chunked_body(move |writer| {
            writer.write_all(bytes)?;
            writer.flush()?;
            let ready = ready.lock().unwrap().take();
            if let Some(ready) = ready {
                ready
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
            Ok(())
        })
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache.clone()));
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
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        resolved["versions"]["1.0.0"]["dist"]["shasum"],
        sha1_hex_of(b"different tarball bytes"),
    );
    assert!(
        resolved["versions"]["1.0.0"]["dist"].get("integrity").is_none(),
        "an integrity over bytes that contradict the declared shasum must not be pinned",
    );
    let blobs = public_cache_blobs(&cache, "foo");
    tokio::time::timeout(Duration::from_secs(10), async {
        while tarball_cache_entries(&blobs).is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background temporary file created");
    release.send(()).unwrap();
    assert!(super::await_no_tgz(&blobs, Duration::from_secs(10)).await);
    assert!(tarball_cache_entries(&public_cache_blobs(&cache, "foo")).is_empty());

    let response = app
        .oneshot(
            Request::get("/npm/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        to_bytes(response.into_body(), usize::MAX).await.is_err(),
        "the tarball stream must abort rather than deliver bytes that contradict the declared shasum",
    );
    assert!(tarball_cache_entries(&public_cache_blobs(&cache, "foo")).is_empty());
    packument_mock.assert_async().await;
    wait_for_mock(&tarball_mock).await;
    tarball_mock.assert_async().await;
}

pub(super) async fn wait_for_pins(storage: &std::path::Path, count: usize) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let path = public_cache_pkg(storage, "foo").join("document.json");
            let doc: Value = serde_json::from_slice(&tokio::fs::read(path).await.unwrap()).unwrap();
            let pinned = doc["versions"]
                .as_object()
                .unwrap()
                .values()
                .filter(|version| has_integrity(version))
                .count();
            let blobs = tarball_cache_entries(&public_cache_blobs(storage, "foo"))
                .iter()
                .filter(|name| name.ends_with(".tgz"))
                .count();
            if pinned >= count && blobs >= count.min(64) {
                return doc;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background integrity pins completed")
}

async fn wait_for_mock(mock: &mockito::Mock) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !mock.matched_async().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background upstream request completed");
}

fn has_integrity(version: &Value) -> bool {
    version
        .pointer("/dist/integrity")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<ssri::Integrity>().ok())
        .is_some_and(|integrity| !integrity.hashes.is_empty())
}
