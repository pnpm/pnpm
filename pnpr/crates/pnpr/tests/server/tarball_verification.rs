use super::{
    Body, Duration, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, config_for,
    foo_packument, json, mock_packument_for_tarball, public_cache_pkg, router, sha1_hex_of,
    sha512_integrity, tarball_cache_entries, to_bytes,
};

#[tokio::test]
async fn tarball_route_preserves_basename_and_binds_to_declaring_version() {
    // A version's tarball is served, fetched, and cached under the basename
    // its own `dist.tarball` declares (preserved verbatim, so a
    // non-canonical upstream name survives into the client's lockfile). The
    // bytes are verified against that declaring version's integrity, so the
    // preserved name still can't smuggle in bytes of another provenance.
    // Here the packument deliberately swaps the two versions' tarball names
    // to prove the binding follows the declaring version, not the filename.
    let mut upstream = mockito::Server::new_async().await;
    let v1_bytes = b"selected-version-one";
    let v2_bytes = b"other-version-two";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-2.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(v1_bytes),
                },
            },
            "2.0.0": {
                "name": "foo",
                "version": "2.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(v2_bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    // `latest` is 1.0.0, whose declared tarball basename is `foo-2.0.0.tgz`,
    // so that is the upstream path pnpr fetches — and it must yield bytes
    // matching 1.0.0's integrity.
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-2.0.0.tgz")
        .with_status(200)
        .with_body(v1_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let selected = app
        .clone()
        .oneshot(
            Request::get("/foo/latest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    let selected: Value = serde_json::from_slice(&body_bytes(selected.into_body()).await).unwrap();
    assert_eq!(selected["dist"]["tarball"], "http://example.test/foo/-/foo-2.0.0.tgz");

    let full = app
        .clone()
        .oneshot(
            Request::get("/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let full: Value = serde_json::from_slice(&body_bytes(full.into_body()).await).unwrap();
    assert_eq!(
        full["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-2.0.0.tgz",
    );
    assert_eq!(
        full["versions"]["2.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );

    let route = selected["dist"]["tarball"]
        .as_str()
        .unwrap()
        .strip_prefix("http://example.test")
        .unwrap();
    let first = app
        .oneshot(
            Request::get(route)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, v1_bytes);

    let package_dir = public_cache_pkg(&storage, "foo");
    assert_eq!(tarball_cache_entries(&package_dir), vec!["foo-2.0.0.tgz".to_string()]);

    // A fresh instance over the same storage and the same origin replays the
    // cached entry; the `expect(1)` mocks prove the upstream is never
    // consulted again. (A *different* URL would be a repoint, which
    // deliberately abandons this cache — see
    // `repointing_an_upstream_url_abandons_the_old_origins_cache`.)
    let restarted = router(config_for(&upstream.url(), storage));
    let replay = restarted
        .oneshot(
            Request::get(route)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_bytes(replay.into_body()).await, v1_bytes);

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn tampered_upstream_tarball_aborts_the_stream_and_is_never_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let good_bytes = b"good-tarball-bytes";
    let poison_bytes = b"poisoned-cache-bytes";
    let packument = json!({
        "name": "poisoned",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "poisoned",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!(
                        "{}/poisoned/-/poisoned-1.0.0.tgz",
                        upstream.url()
                    ),
                    "integrity": sha512_integrity(good_bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/poisoned")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/poisoned/-/poisoned-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(poison_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let cache_path = public_cache_pkg(&storage, "poisoned").join("poisoned-1.0.0.tgz");

    let app = router(config_for(&upstream.url(), storage.clone()));
    let packument_response = app
        .clone()
        .oneshot(
            Request::get("/poisoned")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(packument_response.status(), StatusCode::OK);

    let tarball_response = app
        .oneshot(
            Request::get("/poisoned/-/poisoned-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tarball_response.status(), StatusCode::OK);
    // Draining the body runs the end-of-stream SRI check, which abandons the
    // cache temp on the mismatch.
    assert!(to_bytes(tarball_response.into_body(), usize::MAX).await.is_err());
    assert!(!cache_path.exists(), "unverified tarball must not be written to the cache");

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;

    // Same origin URL (so the same cache namespace), upstream now down: a
    // request must fail rather than be answered from the cache, proving the
    // unverified bytes were never promoted into it.
    let url = upstream.url();
    drop(upstream);
    let cached_response = router(config_for(&url, storage))
        .oneshot(
            Request::get("/poisoned/-/poisoned-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        cached_response.status().is_server_error(),
        "the tampered tarball must not be served from cache, got {}",
        cached_response.status(),
    );
}

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
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    assert_eq!(
        tarball_cache_entries(&public_cache_pkg(&storage, "foo")),
        vec!["foo-1.0.0.tgz".to_string()],
    );
    packument_mock.assert_async().await;
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
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    packument_mock.assert_async().await;
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
            Request::get("/foo")
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
    withdrawn.assert_async().await;
}

/// The downloads can outlast a short `maxage`. A cached packument that went
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
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").maxage =
        Some(Duration::ZERO);
    let response = router(config)
        .oneshot(
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));
}

/// Pins are published into the cached packument. When a refresh cannot be
/// cached, pnpr serves what it fetched and spends no downloads on pins it
/// could not store.
#[tokio::test]
async fn a_refresh_that_fails_to_cache_is_served_unpinned() {
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
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").maxage =
        Some(Duration::ZERO);
    let app = router(config);
    let resolve = || {
        app.clone()
            .oneshot(
                Request::get("/foo")
                    .header("accept", "application/vnd.npm.install-v1+json")
                    .body(Body::empty())
                    .unwrap(),
            )
    };

    let response = resolve().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    // A directory where the packument file belongs makes the next cache write fail.
    let document = public_cache_pkg(&storage, "foo").join("package.json");
    std::fs::remove_file(&document).unwrap();
    std::fs::create_dir_all(document.join("blocker")).unwrap();

    let response = resolve().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(resolved["versions"]["1.0.0"]["dist"].get("integrity").is_none());
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["shasum"], sha1_hex_of(bytes));
    packument_mock.assert_async().await;
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
                Request::get("/foo")
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
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-0.1.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    malformed_tarball.assert_async().await;
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
            Request::get("/foo")
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
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
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
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
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
            Request::get("/foo")
                .header("accept", "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
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
            Request::get("/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let resolved: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(resolved["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(bytes));

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    packument_mock.assert_async().await;
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
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache.clone()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/foo")
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
    assert!(tarball_cache_entries(&public_cache_pkg(&cache, "foo")).is_empty());

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        to_bytes(response.into_body(), usize::MAX).await.is_err(),
        "the tarball stream must abort rather than deliver bytes that contradict the declared shasum",
    );
    assert!(tarball_cache_entries(&public_cache_pkg(&cache, "foo")).is_empty());
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn ambiguous_tarball_basename_is_rejected_before_fetch() {
    // Two versions declaring the same dist.tarball basename make the
    // declaring version ambiguous, so the request must fail closed rather
    // than bind integrity/OSV to whichever version is encountered first.
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"shared-basename-bytes";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(bytes),
                },
            },
            "2.0.0": {
                "name": "foo",
                "version": "2.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(bytes),
                },
            },
        },
    });
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
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

/// An integrity pnpr can neither use nor replace is a controlled failure: the
/// tarball is never fetched. A value that parses to no hashes is not in this
/// set, because it pins nothing and is computed instead.
#[tokio::test]
async fn invalid_tarball_integrities_are_controlled_failures() {
    for (case, integrity) in [("malformed", "not-a-valid-sri"), ("unsupported", "md5-deadbeef")] {
        let mut upstream = mockito::Server::new_async().await;
        let packument = json!({
            "name": "foo",
            "versions": {
                "1.0.0": {
                    "name": "foo",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                        "integrity": integrity,
                    },
                },
            },
        });
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
            .with_body("must-not-be-fetched")
            .expect(0)
            .create_async()
            .await;

        let tmp = TempDir::new().unwrap();
        let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
            .oneshot(
                Request::get("/foo/-/foo-1.0.0.tgz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "case: {case}");
        packument_mock.assert_async().await;
        tarball_mock.assert_async().await;
    }
}

#[tokio::test]
async fn tarball_verification_finalizes_cache_with_no_tmp_leftover() {
    let mut upstream = mockito::Server::new_async().await;
    // Large-ish body so the streaming path is exercised across many
    // chunks rather than fitting in a single hyper buffer.
    let bytes = vec![0xAB_u8; 512 * 1024];
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "big", "1.0.0", &bytes).await;
    let _mock = upstream
        .mock("GET", "/big/-/big-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

    let response = app
        .oneshot(
            Request::get("/big/-/big-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let received = body_bytes(response.into_body()).await;
    assert_eq!(received.len(), bytes.len());
    assert_eq!(received, bytes);

    // Verification and finalization complete before the response is
    // built, leaving only the canonical cache path.
    let package_dir = public_cache_pkg(&cache_dir, "big");
    let entries = tarball_cache_entries(&package_dir);
    assert_eq!(entries, vec!["big-1.0.0.tgz".to_string()]);

    let cached = std::fs::read(package_dir.join("big-1.0.0.tgz")).unwrap();
    assert_eq!(cached.len(), bytes.len());
    assert_eq!(cached, bytes);
}

#[tokio::test]
async fn cache_false_upstream_rejects_tampered_tarball_without_mirroring() {
    let mut upstream = mockito::Server::new_async().await;
    let good_bytes = b"good-uncached-tarball";
    let poison_bytes = b"poisoned-uncached-tarball";
    let packument_mock =
        mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", good_bytes).await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(poison_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let mut config = config_for(&upstream.url(), cache_dir.clone());
    config.routing.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").cache = false;
    let app = router(config);

    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let package_dir = public_cache_pkg(&cache_dir, "foo");
    assert!(
        tarball_cache_entries(&package_dir).is_empty(),
        "a rejected cache:false tarball must leave no mirror entry",
    );

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}
