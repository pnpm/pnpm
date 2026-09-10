use super::{
    Body, Duration, Request, ServiceExt, StatusCode, TempDir, await_no_tgz, body_bytes, config_for,
    json, mock_packument_for_tarball, public_cache_pkg, router, sha512_integrity,
    spawn_truncated_upstream, tarball_cache_entries,
};

/// Stale-if-error: once a cached packument is stale, a refetch that fails
/// (transient upstream error) falls back to the last cached body rather than
/// erroring — preserving availability without a cross-origin fall-through.
#[tokio::test]
async fn stale_packument_is_served_when_upstream_refetch_fails() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0" } },
    });
    let ok = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.packument_ttl = Duration::from_millis(50);
    let app = router(config);

    // Prime the cache with a fresh fetch.
    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;
    ok.assert_async().await;
    ok.remove_async().await;

    // The entry goes stale and the upstream now fails every refetch. Staleness
    // is decided from the cache file's mtime, so sleep well past a coarse
    // filesystem's timestamp granularity (~1s) plus the TTL — otherwise the
    // entry could still read as fresh and the test would flake.
    let boom =
        upstream.mock("GET", "/foo").with_status(500).expect_at_least(1).create_async().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // The stale cached packument is served (200), not a 502.
    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let body = body_bytes(r2.into_body()).await;
    assert!(
        String::from_utf8_lossy(&body).contains("1.0.0"),
        "stale packument body should still carry its versions",
    );
    boom.assert_async().await;
}

/// Stale-if-error masks only *transient* failures: a 4xx (here `403`) is
/// authoritative about this request, so the cached bytes must NOT be served —
/// the error surfaces instead.
#[tokio::test]
async fn a_4xx_upstream_does_not_serve_stale_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "dist-tags": { "latest": "1.0.0" }, "versions": {} });
    let ok = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.packument_ttl = Duration::from_millis(50);
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;
    ok.assert_async().await;
    ok.remove_async().await;

    // The refetch now returns 403 (auth revoked). It must not be masked.
    let denied =
        upstream.mock("GET", "/foo").with_status(403).expect_at_least(1).create_async().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_ne!(r2.status(), StatusCode::OK, "a 4xx must not be answered from stale cache");
    denied.assert_async().await;
}

#[tokio::test]
async fn concurrent_tarball_fetches_settle_to_one_cache_file() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = vec![0xCD; 128 * 1024];
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", &bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        .expect_at_least(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::OK);

    let req1 =
        app.clone().oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap());
    let req2 =
        app.clone().oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap());
    let (r1, r2) = tokio::join!(req1, req2);
    let (r1, r2) = (r1.unwrap(), r2.unwrap());
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(body_bytes(r1.into_body()).await, bytes);
    assert_eq!(body_bytes(r2.into_body()).await, bytes);

    // Any overlapping verified writes atomically target the same final
    // path, so one tarball remains with no temporary siblings.
    let dir = public_cache_pkg(&cache_dir, "foo");
    let entries = tarball_cache_entries(&dir);
    assert_eq!(entries, vec!["foo-1.0.0.tgz".to_string()]);

    mock.assert_async().await;
}

#[tokio::test]
async fn cache_tmp_open_failure_fails_closed() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"served-without-cache";
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let _mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .create_async()
        .await;

    // Point `cache_dir` at a regular file so `create_dir_all` inside
    // `open_cached_tarball_tmp` fails. Without a place to verify the
    // complete body, the handler must fail rather than stream it.
    let tmp = TempDir::new().unwrap();
    let blocked = tmp.path().join("not-a-dir");
    std::fs::write(&blocked, b"already a file").unwrap();

    let app = router(config_for(&upstream.url(), blocked.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    // The cache path under the not-a-dir should not exist.
    let cache_path = public_cache_pkg(&blocked, "foo").join("foo-1.0.0.tgz");
    assert!(!cache_path.exists());
}

#[tokio::test]
async fn malformed_upstream_json_maps_to_bad_gateway() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/borked")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("<html>upstream CDN error page</html>")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/borked").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn upstream_stream_error_clears_cache() {
    let expected_bytes = vec![0xAA; 1024 * 1024];
    let addr = spawn_truncated_upstream(sha512_integrity(&expected_bytes)).await;
    let upstream_url = format!("http://{addr}");
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream_url, cache_dir.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // The status line is sent before the upstream truncation is known, so the
    // failure surfaces as a body error mid-stream rather than a status code.
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        axum::body::to_bytes(response.into_body(), usize::MAX).await.is_err(),
        "a truncated upstream body must surface as a body error",
    );

    // The incomplete body is never promoted to the cache.
    assert!(await_no_tgz(&public_cache_pkg(&cache_dir, "foo"), Duration::from_secs(1)).await);
}

#[tokio::test]
async fn proxied_tarball_streams_to_client_and_is_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = vec![0xEE_u8; 512 * 1024];
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
    // The client receives the body as it streams; draining it runs the
    // end-of-stream SRI check that promotes the verified bytes to the cache.
    assert_eq!(body_bytes(response.into_body()).await, bytes);

    let cache_path = public_cache_pkg(&cache_dir, "big").join("big-1.0.0.tgz");
    assert_eq!(std::fs::read(cache_path).unwrap(), bytes);
}

#[tokio::test]
async fn tarball_is_not_gzipped_even_when_accepted() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes-long-enough-to-clear-the-compression-size-floor";
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // Tarballs are already `.tgz` (gzip); the layer must not re-compress
    // them even when the client offers `Accept-Encoding: gzip`.
    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .header("accept-encoding", "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("content-encoding").is_none(),
        "tarballs must not be re-gzipped",
    );
    assert_eq!(body_bytes(response.into_body()).await, bytes);

    mock.assert_async().await;
}

/// A per-upstream `maxage` shorter than the global `packument_ttl` governs
/// freshness: with a generous global TTL the second request would be a
/// cache hit, but `maxage: 0` forces a revalidation, so the upstream is
/// hit twice.
#[tokio::test]
async fn per_upstream_maxage_overrides_global_packument_ttl() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "versions": {} });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // Global TTL stays generous (a minute); the per-upstream maxage of zero
    // is what must take effect and make every read stale.
    config.packument_ttl = Duration::from_mins(1);
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").maxage =
        Some(Duration::from_millis(0));
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    mock.assert_async().await;
}

/// A `cache: false` upstream streams tarballs through without writing them
/// to the local mirror: a second request re-fetches from the upstream
/// (so the mock is hit twice) and no `.tgz` is left in the cache dir.
#[tokio::test]
async fn cache_false_upstream_streams_tarball_without_mirroring() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"uncached-tarball-bytes";
    // A `cache: false` registry streams everything through, so each of the two
    // requests refetches the packument as well as the tarball.
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let mut config = config_for(&upstream.url(), cache_dir.clone());
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").cache = false;
    let app = router(config);

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }

    // Nothing was mirrored: the package dir either doesn't exist or holds
    // no tarball or temp tarball. Both requests therefore went to the upstream.
    let package_dir = public_cache_pkg(&cache_dir, "foo");
    assert!(
        tarball_cache_entries(&package_dir).is_empty(),
        "a cache:false upstream must not write tarballs to the mirror",
    );

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

/// A definitive upstream 404 purges the cached packument, so a package
/// unpublished upstream cannot be resurrected later by the stale-if-error
/// fallback during a transient outage.
#[tokio::test]
async fn upstream_404_purges_cached_packument() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "versions": {} });
    let ok_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.packument_ttl = Duration::from_millis(50);
    let app = router(config);

    let first =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let _ = body_bytes(first.into_body()).await;
    ok_mock.assert_async().await;
    let cached = public_cache_pkg(tmp.path(), "foo");
    assert!(cached.exists(), "first fetch should cache the packument");

    // The package is unpublished upstream; past the TTL the refetch sees the
    // authoritative 404 and must drop the cached entry.
    ok_mock.remove_async().await;
    upstream.mock("GET", "/foo").with_status(404).create_async().await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    let gone =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    assert!(!cached.exists(), "the 404 must purge the cached packument");

    // A later transient outage can no longer resurrect it from stale cache.
    let dead = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let outage = dead.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_ne!(outage.status(), StatusCode::OK, "purged package must not be served stale");
}
