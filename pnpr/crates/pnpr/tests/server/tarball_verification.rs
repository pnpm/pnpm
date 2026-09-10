use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, config_for, foo_packument,
    json, mock_packument_for_tarball, public_cache_pkg, router, sha1_hex_of, sha512_integrity,
    tarball_cache_entries, to_bytes,
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
        .oneshot(Request::get("/foo/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    let selected: Value = serde_json::from_slice(&body_bytes(selected.into_body()).await).unwrap();
    assert_eq!(selected["dist"]["tarball"], "http://example.test/foo/-/foo-2.0.0.tgz");

    let full =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    let full: Value = serde_json::from_slice(&body_bytes(full.into_body()).await).unwrap();
    assert_eq!(
        full["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-2.0.0.tgz",
    );
    assert_eq!(
        full["versions"]["2.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );

    let route =
        selected["dist"]["tarball"].as_str().unwrap().strip_prefix("http://example.test").unwrap();
    let first = app.oneshot(Request::get(route).body(Body::empty()).unwrap()).await.unwrap();
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
    let replay = restarted.oneshot(Request::get(route).body(Body::empty()).unwrap()).await.unwrap();
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
    let packument_response =
        app.clone().oneshot(Request::get("/poisoned").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument_response.status(), StatusCode::OK);

    let tarball_response = app
        .oneshot(Request::get("/poisoned/-/poisoned-1.0.0.tgz").body(Body::empty()).unwrap())
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
        .oneshot(Request::get("/poisoned/-/poisoned-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(
        cached_response.status().is_server_error(),
        "the tampered tarball must not be served from cache, got {}",
        cached_response.status(),
    );
}

#[tokio::test]
async fn tarball_without_integrity_or_shasum_is_rejected_before_fetch() {
    let mut upstream = mockito::Server::new_async().await;
    // No `dist.integrity` and no `dist.shasum`: nothing to verify the bytes
    // against, so the request must fail before any tarball fetch.
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"].as_object_mut().unwrap().remove("shasum");
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
        .with_body("unverified")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

/// A pre-2017 npm publish carries only the legacy hex `dist.shasum`. It must
/// stay proxyable — verified against sha1 rather than not served at all.
#[tokio::test]
async fn shasum_only_tarball_is_served_with_sha1_verification() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"legacy-shasum-tarball";
    let mut packument = foo_packument(&upstream.url());
    packument["versions"]["1.0.0"]["dist"]["shasum"] = json!(sha1_hex_of(bytes));
    upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
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
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
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
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn invalid_tarball_integrities_are_controlled_failures() {
    for (case, integrity) in [
        ("malformed", "not-a-valid-sri"),
        ("whitespace", " \t\n "),
        ("zero-hash", ""),
        ("unsupported", "md5-deadbeef"),
    ] {
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
            .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
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
        .oneshot(Request::get("/big/-/big-1.0.0.tgz").body(Body::empty()).unwrap())
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
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").cache = false;
    let app = router(config);

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
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
