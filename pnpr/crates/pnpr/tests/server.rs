use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use flate2::read::GzDecoder;
use pnpr::{Config, router};
use serde_json::{Value, json};
use std::{
    io::Read as _,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

fn config_for(upstream: &str, storage: std::path::PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").url = upstream.to_string();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

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
async fn uplink_auth_and_custom_headers_are_forwarded_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    // The mock only matches when both headers are present, so a passing
    // request proves the resolved per-uplink headers reached upstream.
    let mock = upstream
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": {} }).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let uplink = config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink");
    uplink.headers.insert("authorization", "Bearer secret-token".parse().unwrap());
    uplink.headers.insert("x-org", "acme".parse().unwrap());
    let app = router(config);

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn scoped_packument_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "@types/node", "versions": {} });
    let mock = upstream
        .mock("GET", "/@types/node")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/@types/node").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_is_proxied_and_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes";
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

    let first = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, bytes);

    let second = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, bytes);

    mock.assert_async().await;
}

#[tokio::test]
async fn upstream_404_is_propagated() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/missing")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/missing").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tarball_streaming_finalizes_cache_with_no_tmp_leftover() {
    let mut upstream = mockito::Server::new_async().await;
    // Large-ish body so the streaming path is exercised across many
    // chunks rather than fitting in a single hyper buffer.
    let bytes = vec![0xAB_u8; 512 * 1024];
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

    // By the time the client's stream ends (rx sees None), the tee
    // task has already called finalize, so the cache file is in
    // place at the canonical path and no `.tmp.*` siblings remain.
    // Proxied tarballs land in the disposable cache root.
    let package_dir = cache_dir.join(".pnpr-cache").join("big");
    let entries: Vec<String> = std::fs::read_dir(&package_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tgz"))
        .collect();
    assert_eq!(entries, vec!["big-1.0.0.tgz".to_string()]);

    let cached = std::fs::read(package_dir.join("big-1.0.0.tgz")).unwrap();
    assert_eq!(cached.len(), bytes.len());
    assert_eq!(cached, bytes);
}

#[tokio::test]
async fn upstream_5xx_maps_to_bad_gateway() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock =
        upstream.mock("GET", "/broken").with_status(500).with_body("kaboom").create_async().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/broken").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn unreachable_upstream_maps_to_service_unavailable() {
    // Bind a TCP listener and immediately drop it so the port is
    // (very likely) free; pointing the registry at a port nothing is
    // listening on exercises the `is_connect()` branch of the status
    // mapping without depending on DNS.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    let dead_upstream = format!("http://127.0.0.1:{dead_port}");

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&dead_upstream, tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn tarball_filename_for_other_package_is_rejected() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::get("/foo/-/bar-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn scoped_tarball_is_proxied() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-tarball-bytes";
    let mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::get("/@types/node/-/node-20.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
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

/// A stale cached packument is revalidated with a conditional GET: the
/// upstream's `ETag` is replayed as `If-None-Match`, and a `304` lets
/// the server serve its cached copy without re-downloading the body. The
/// `304` also refreshes the entry, so a third request inside the TTL is
/// served straight from cache with no upstream call at all.
#[tokio::test]
async fn stale_packument_is_revalidated_with_conditional_get() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "dist-tags": { "latest": "1.0.0" }, "versions": {} });
    // First request (no validator yet): full body plus an ETag to store.
    let full = upstream
        .mock("GET", "/foo")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("etag", r#""v1""#)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    // Revalidation: the stored ETag comes back as If-None-Match; upstream
    // confirms it's unchanged with a bodyless 304.
    let revalidate = upstream
        .mock("GET", "/foo")
        .match_header("if-none-match", r#""v1""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // A generous TTL: r2's `304` refresh rewrites the cache file, and r3 must
    // see that entry as fresh. The margin has to comfortably exceed the wall
    // time between r2's refresh-write and r3's freshness check, which can spike
    // on a contended CI runner (and is subject to coarse filesystem mtime
    // granularity on Windows). The r1->r2 sleep stays longer than the TTL so r2
    // is still stale.
    config.packument_ttl = Duration::from_millis(500);
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Wait past the TTL so the cached packument is stale and gets revalidated.
    tokio::time::sleep(Duration::from_millis(700)).await;

    let r2 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(r2.into_body()).await).unwrap();
    assert_eq!(body["dist-tags"]["latest"], "1.0.0", "304 must serve the cached body");

    // The 304 refreshed the entry, so this request is within the TTL and
    // never reaches the upstream — both mocks asserting exactly one call.
    let r3 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r3.status(), StatusCode::OK);

    full.assert_async().await;
    revalidate.assert_async().await;
}

/// A hosted packument is authoritative: even with a proxy upstream
/// configured, a divergent upstream packument, and an expired TTL, the
/// hosted copy is served verbatim and the upstream is never contacted.
/// This is the guarantee that published versions can't be masked or
/// lost by a proxy refresh.
#[tokio::test]
async fn hosted_packument_is_never_overwritten_by_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let upstream_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": { "9.9.9": {} } }).to_string())
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();

    // Seed the hosted store directly, as a publish (or static fixture)
    // would have. The hosted root is `config.storage` == tmp.
    let hosted = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
                "tarball": "http://example.test/foo/-/foo-1.0.0.tgz", "shasum": "abc"
            }},
        },
    });
    std::fs::create_dir_all(tmp.path().join("foo")).unwrap();
    std::fs::write(tmp.path().join("foo/package.json"), hosted.to_string()).unwrap();

    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // Zero TTL: if the hosted copy were treated as a cache entry, this
    // would force an immediate upstream refetch.
    config.packument_ttl = Duration::from_millis(0);
    let app = router(config);

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let versions: Vec<&str> =
        body["versions"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(versions, vec!["1.0.0"], "hosted packument must win over upstream's 9.9.9");

    // The upstream was never hit — the hosted copy short-circuits the proxy.
    upstream_mock.assert_async().await;
}

#[tokio::test]
async fn stale_packument_is_served_when_upstream_fails() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "dist-tags": { "latest": "1.0.0" }, "versions": {} });
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();

    // Round 1: warm the cache against a working upstream.
    let r1 = router(config_for(&upstream.url(), cache_dir.clone()))
        .oneshot(Request::get("/foo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Round 2: point at a dead port (so upstream errors) and set TTL
    // to zero so the cache is considered stale. The handler should
    // try upstream, fail, and fall back to the on-disk packument.
    let mut dead_config = config_for("http://127.0.0.1:1", cache_dir.clone());
    dead_config.packument_ttl = Duration::from_millis(0);
    let r2 = router(dead_config)
        .oneshot(Request::get("/foo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK, "stale cache should be served on upstream failure");
    let body: Value = serde_json::from_slice(&body_bytes(r2.into_body()).await).unwrap();
    assert_eq!(body["name"], "foo");
    assert_eq!(body["dist-tags"]["latest"], "1.0.0");
}

#[tokio::test]
async fn invalid_package_name_returns_bad_request() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // `.hidden` trips the dot-prefix rejection in `PackageName::parse`.
    let response =
        app.oneshot(Request::get("/.hidden").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn concurrent_tarball_fetches_settle_to_one_cache_file() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = vec![0xCD; 128 * 1024];
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        // We deliberately don't single-flight, so the upstream is hit
        // at least twice. Bound only the lower side; the exact count
        // depends on scheduling.
        .expect_at_least(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

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

    // After both responses drain, both tee tasks have called
    // `finalize` (rename is last-writer-wins on POSIX, and both
    // wrote identical content). Exactly one `.tgz` file in the
    // package dir, no `.tmp.*` survivors thanks to the unique-tmp
    // suffix. Proxied tarballs live in the disposable cache root.
    let dir = cache_dir.join(".pnpr-cache").join("foo");
    let entries: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tgz"))
        .collect();
    assert_eq!(entries, vec!["foo-1.0.0.tgz".to_string()]);

    mock.assert_async().await;
}

#[tokio::test]
async fn cache_tmp_open_failure_falls_back_to_uncached_stream() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"served-without-cache";
    let _mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .create_async()
        .await;

    // Point `cache_dir` at a regular file so `create_dir_all` inside
    // `open_cached_tarball_tmp` fails. The handler should still stream
    // the body to the client and skip the cache write.
    let tmp = TempDir::new().unwrap();
    let blocked = tmp.path().join("not-a-dir");
    std::fs::write(&blocked, b"already a file").unwrap();

    let app = router(config_for(&upstream.url(), blocked.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);

    // The cache path under the not-a-dir should not exist.
    let cache_path = blocked.join(".pnpr-cache").join("foo").join("foo-1.0.0.tgz");
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

/// Spawn a TCP listener that speaks just enough HTTP/1.1 to answer a
/// single GET with valid headers and a *truncated* body, then drops
/// the connection. Mockito can't simulate mid-body disconnects, so a
/// hand-rolled server is the cheapest way to exercise the `upstream
/// stream errored mid-body` branch in `run_tee`.
async fn spawn_truncated_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\n\
                          Content-Length: 1048576\r\n\
                          Content-Type: application/octet-stream\r\n\
                          Connection: close\r\n\
                          \r\n",
                    )
                    .await;
                let _ = socket.write_all(&[0xAA; 100]).await;
                // Drop socket without sending the remaining 1048476 bytes.
            });
        }
    });
    addr
}

#[tokio::test]
async fn upstream_stream_error_clears_cache() {
    let addr = spawn_truncated_upstream().await;
    let upstream_url = format!("http://{addr}");
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream_url, cache_dir.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Body read should fail — upstream advertised 1 MiB and closed
    // the socket after 100 bytes.
    let result = to_bytes(response.into_body(), usize::MAX).await;
    assert!(result.is_err(), "expected body to error mid-stream");

    // The tee task observes the upstream error and calls
    // `write.abandon()`. We don't know when that finishes, but it
    // should happen quickly — poll for it so the test fails fast on
    // success and gives a 1s budget for the worst case (heavy CI
    // load).
    assert!(await_no_tgz(&cache_dir.join(".pnpr-cache").join("foo"), Duration::from_secs(1)).await);
}

#[tokio::test]
async fn client_disconnect_mid_stream_clears_cache() {
    let mut upstream = mockito::Server::new_async().await;
    // 8 MiB is comfortably larger than the tee channel can buffer
    // (16 chunks × ~64 KiB ≈ 1 MiB), so the tee task is guaranteed
    // to be parked on `tx.send` when we drop the response below.
    let bytes = vec![0xEE_u8; 8 * 1024 * 1024];
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

    // Drop the response without reading the body. The mpsc receiver
    // goes away, the tee task's next `tx.send` returns Err, and
    // `write.abandon()` runs. Poll for the abandon so the test
    // doesn't depend on a fixed sleep budget under heavy CI load.
    drop(response);
    assert!(await_no_tgz(&cache_dir.join(".pnpr-cache").join("big"), Duration::from_secs(1)).await);
}

/// Poll `dir` until it contains no `.tgz` files *and* no `.tmp.*`
/// orphans (or doesn't exist), up to `budget`. Returns `true` on
/// success, `false` on timeout — gives the calling test a single
/// deterministic signal that the tee task observed the abandon
/// condition and `write.abandon()` actually unlinked the temp file.
async fn await_no_tgz(dir: &std::path::Path, budget: Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        let still_present = dir.read_dir().is_ok_and(|iter| {
            iter.filter_map(Result::ok).any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.ends_with(".tgz") || name.contains(".tmp.")
            })
        });
        if !still_present {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn ping_endpoint_returns_json_empty_object() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    let app = router(config);

    let response = app.oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = body_bytes(response.into_body()).await;
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body, json!({}));
}

fn foo_packument(upstream_url: &str) -> Value {
    json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{upstream_url}/foo/-/foo-1.0.0.tgz"),
                    "shasum": "deadbeef",
                },
            },
        },
    })
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

#[tokio::test]
async fn tarball_is_not_gzipped_even_when_accepted() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes-long-enough-to-clear-the-compression-size-floor";
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
