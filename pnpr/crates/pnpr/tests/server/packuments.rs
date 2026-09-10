use super::{
    Body, Duration, GzDecoder, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes,
    body_json, config_for, enable_osv, foo_packument, json, osv_database, router,
};
use std::io::Read;

#[tokio::test]
async fn packument_is_proxied_cached_and_rewritten() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "shasum": "deadbeef"
                }
            }
        }
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.public_url = "http://example.test".to_string();
    let app = router(config);

    let response =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(body["versions"]["1.0.0"]["dist"]["shasum"], "deadbeef");

    let cached =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);

    packument_mock.assert_async().await;
}

#[tokio::test]
async fn packument_responses_carry_last_modified_for_head_probes() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "time": { "modified": "2026-06-21T12:00:00.500Z" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()) },
            }
        }
    });
    let _packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let bare = json!({
        "name": "bare",
        "versions": {
            "1.0.0": {
                "name": "bare",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/bare/-/bare-1.0.0.tgz", upstream.url()) },
            }
        }
    });
    let _bare_mock = upstream
        .mock("GET", "/bare")
        .with_status(200)
        .with_body(bare.to_string())
        .create_async()
        .await;
    let garbled = json!({
        "name": "garbled",
        "time": { "modified": "not-a-date" },
        "versions": {
            "1.0.0": {
                "name": "garbled",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/garbled/-/garbled-1.0.0.tgz", upstream.url()) },
            }
        }
    });
    let _garbled_mock = upstream
        .mock("GET", "/garbled")
        .with_status(200)
        .with_body(garbled.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let app = router(config);

    // The fractional `time.modified` rounds *up*: the header must stay
    // an upper bound on the publish time for release-age checks.
    let expected = "Sun, 21 Jun 2026 12:00:01 GMT";
    let get = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(
        get.headers().get("last-modified").and_then(|value| value.to_str().ok()),
        Some(expected),
    );

    // The same route answers `HEAD` (hyper strips the body on the wire),
    // so a client can read the bound without downloading the document.
    let head = app
        .clone()
        .oneshot(Request::builder().method("HEAD").uri("/foo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(
        head.headers().get("last-modified").and_then(|value| value.to_str().ok()),
        Some(expected),
    );

    // A document without a parsable `time.modified` omits the header
    // instead of guessing — absent and garbled values alike.
    let no_time =
        app.clone().oneshot(Request::get("/bare").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(no_time.status(), StatusCode::OK);
    assert!(no_time.headers().get("last-modified").is_none());
    let unparsable =
        app.clone().oneshot(Request::get("/garbled").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(unparsable.status(), StatusCode::OK);
    assert!(unparsable.headers().get("last-modified").is_none());
}

#[tokio::test]
async fn osv_filters_vulnerable_versions_from_proxy_and_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.1.0", "stable": "1.0.0" },
        "time": {
            "modified": "2026-06-21T12:00:00.000Z",
            "1.0.0": "2026-06-20T12:00:00.000Z",
            "1.1.0": "2026-06-21T12:00:00.000Z",
        },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()) },
            },
            "1.1.0": {
                "name": "foo",
                "version": "1.1.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.1.0.tgz", upstream.url()) },
            },
        },
    });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.1.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let first =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let body = body_json(first.into_body()).await;
    assert!(body["versions"].get("1.1.0").is_none());
    assert!(body["time"].get("1.1.0").is_none());
    assert_eq!(body["versions"]["1.0.0"]["version"], "1.0.0");
    assert!(body["dist-tags"].get("latest").is_none());
    assert_eq!(body["dist-tags"]["stable"], "1.0.0");

    let cached =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);
    let cached_body = body_json(cached.into_body()).await;
    assert!(cached_body["versions"].get("1.1.0").is_none());
    assert!(cached_body["time"].get("1.1.0").is_none());
    assert!(cached_body["dist-tags"].get("latest").is_none());
    assert_eq!(cached_body["dist-tags"]["stable"], "1.0.0");

    let vulnerable_manifest =
        app.clone().oneshot(Request::get("/foo/1.1.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(vulnerable_manifest.status(), StatusCode::NOT_FOUND);

    let safe_manifest =
        app.clone().oneshot(Request::get("/foo/1.0.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(safe_manifest.status(), StatusCode::OK);
    let safe_body = body_json(safe_manifest.into_body()).await;
    assert_eq!(safe_body["version"], "1.0.0");

    let dist_tags = app
        .oneshot(Request::get("/-/package/foo/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(dist_tags.status(), StatusCode::OK);
    let tags = body_json(dist_tags.into_body()).await;
    assert!(tags.get("latest").is_none());
    assert_eq!(tags["stable"], "1.0.0");

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_filters_packument_identity_mismatches() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": {
            "latest": "1.1.0",
            "alias": "safe-key",
            "hidden": "1.2.0",
            "stable": "1.0.0",
        },
        "time": {
            "modified": "2026-06-21T12:00:00.000Z",
            "1.0.0": "2026-06-20T12:00:00.000Z",
            "1.1.0": "2026-06-21T12:00:00.000Z",
            "safe-key": "2026-06-21T12:00:00.000Z",
            "1.2.0": "2026-06-21T12:00:00.000Z",
        },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()) },
            },
            "1.1.0": {
                "name": "foo",
                "version": "9.9.9",
                "dist": { "tarball": format!("{}/foo/-/foo-1.1.0.tgz", upstream.url()) },
            },
            "safe-key": {
                "name": "foo",
                "version": "1.2.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.2.0.tgz", upstream.url()) },
            },
        },
    });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.1.0", "1.2.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let versions = body["versions"].as_object().unwrap();
    assert_eq!(versions.len(), 1);
    assert!(versions.contains_key("1.0.0"));
    let tags = body["dist-tags"].as_object().unwrap();
    assert_eq!(tags.len(), 1);
    assert!(tags.contains_key("stable"));
    let time = body["time"].as_object().unwrap();
    assert_eq!(time.len(), 2);
    assert!(time.contains_key("modified"));
    assert!(time.contains_key("1.0.0"));

    let vulnerable_key_manifest =
        app.clone().oneshot(Request::get("/foo/1.1.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(vulnerable_key_manifest.status(), StatusCode::NOT_FOUND);
    let vulnerable_manifest_version = app
        .clone()
        .oneshot(Request::get("/foo/safe-key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(vulnerable_manifest_version.status(), StatusCode::NOT_FOUND);

    let dist_tags = app
        .oneshot(Request::get("/-/package/foo/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(dist_tags.status(), StatusCode::OK);
    let tags = body_json(dist_tags.into_body()).await;
    assert_eq!(tags.as_object().unwrap().len(), 1);
    assert_eq!(tags["stable"], "1.0.0");

    mock.assert_async().await;
}

#[tokio::test]
async fn packument_is_refetched_after_ttl_expires() {
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
    config.packument_ttl = Duration::from_millis(50);
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Wait past the TTL so the cached packument is stale.
    tokio::time::sleep(Duration::from_millis(120)).await;

    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    // Mock asserts exactly 2 upstream calls were made.
    mock.assert_async().await;
}

#[tokio::test]
async fn packument_is_gzipped_for_clients_that_accept_it() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(foo_packument(&upstream.url()).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(
            Request::get("/foo").header("accept-encoding", "gzip").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-encoding").and_then(|value| value.to_str().ok()),
        Some("gzip"),
        "a packument should be gzipped when the client accepts gzip",
    );

    // Decoding yields the same rewritten JSON a plain request would return.
    let gzipped = body_bytes(response.into_body()).await;
    let mut decoded = Vec::new();
    GzDecoder::new(&gzipped[..]).read_to_end(&mut decoded).expect("decode gzip");
    let body: Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn packument_is_not_gzipped_without_accept_encoding() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(foo_packument(&upstream.url()).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("content-encoding").is_none(),
        "no Accept-Encoding means the packument is served uncompressed",
    );
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["name"], "foo");

    mock.assert_async().await;
}
