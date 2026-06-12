use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use tempfile::TempDir;

use super::{FetchFullMetadataCachedOptions, fetch_full_metadata_cached};
use crate::mirror::{FULL_META_DIR, get_pkg_mirror_path, load_meta, load_meta_headers};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.0.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": { "1.0.0": "2025-01-10T08:30:00.000Z" },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        }
    }
}"#;

fn no_retry_opts() -> RetryOpts {
    RetryOpts { retries: 0, ..Default::default() }
}

/// Cold cache (no mirror file) → registry returns 200 → mirror is
/// populated with the response body + etag.
#[tokio::test]
async fn cold_cache_writes_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    assert!(mirror_path.exists(), "mirror file written");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"fresh""#));
}

/// Warm cache + matching `If-None-Match` → 304 → response served
/// from disk; no parse against the empty body.
#[tokio::test]
async fn warm_cache_serves_from_mirror_on_304() {
    let mut server = mockito::Server::new_async().await;
    // First call: 200, mirror written.
    let first = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    // Second call: must carry If-None-Match: W/"v1" — registry replies 304.
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"v1""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let _first_pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 populates cache");
    first.assert_async().await;

    let second_pkg =
        fetch_full_metadata_cached("acme", &opts).await.expect("304 reads from mirror");
    second.assert_async().await;
    assert_eq!(second_pkg.name, "acme");
    assert_eq!(second_pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
}

/// Warm cache + stale `If-None-Match` → 200 → mirror is overwritten
/// with the new body + new etag.
#[tokio::test]
async fn stale_cache_refreshes_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let updated_body = PACKAGE_BODY.replace("2025-01-10T08:30:00.000Z", "2025-03-01T00:00:00.000Z");
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"v1""#)
        .with_status(200)
        .with_header("etag", r#"W/"v2""#)
        .with_body(updated_body)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let _ = fetch_full_metadata_cached("acme", &opts).await.expect("populate");
    first.assert_async().await;

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("refresh");
    second.assert_async().await;
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-03-01T00:00:00.000Z"));
    let mirror = get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    let reloaded = load_meta(&mirror).expect("mirror readable");
    assert_eq!(reloaded.etag.as_deref(), Some(r#"W/"v2""#));
}

/// `cache_dir = None` → straight unconditional GET, no mirror IO. A
/// 200 response yields the parsed package as before; nothing is
/// written to disk.
#[tokio::test]
async fn no_cache_dir_skips_mirror_io() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: None,
        full_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;
}

/// `cache_dir` points at a read-only directory. The fetch still
/// succeeds with the parsed body — cache writes are fire-and-forget,
/// failures only suppress the next-install speedup. Mirrors
/// upstream's `saveMeta(...).catch(() => {})`.
#[cfg(unix)]
#[tokio::test]
async fn read_only_cache_dir_does_not_fail_the_call() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let mode = cache.path().metadata().expect("stat").permissions().mode();
    fs::set_permissions(cache.path(), fs::Permissions::from_mode(0o555)).expect("set read-only");

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("read-only must not fail");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    // Restore so TempDir's drop can clean up.
    let _ = fs::set_permissions(cache.path(), fs::Permissions::from_mode(mode));
}
