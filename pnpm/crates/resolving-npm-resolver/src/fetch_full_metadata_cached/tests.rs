use mockito::Matcher;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use tempfile::TempDir;

use super::{FetchFullMetadataCachedOptions, fetch_full_metadata_cached};
use crate::{
    FetchMetadataError,
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, get_pkg_mirror_path,
        load_meta, load_meta_headers, save_meta_indexed,
    },
};

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

fn fast_retry_opts() -> RetryOpts {
    RetryOpts {
        retries: 1,
        min_timeout: std::time::Duration::from_millis(1),
        max_timeout: std::time::Duration::from_millis(1),
        ..Default::default()
    }
}

/// A `Content-Encoding: gzip` header over a body that isn't gzip makes
/// reqwest fail while decoding the response body — the same class of
/// failure as a connection reset mid-transfer, which `send_with_retry`
/// can't see because it happens after the request returns `200`.
async fn corrupt_gzip_body_mock(server: &mut mockito::ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("content-encoding", "gzip")
        .with_body("this is not valid gzip")
        .expect(1)
        .create_async()
        .await
}

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
        filter_metadata: false,
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

#[tokio::test]
async fn unsolicited_304_retries_without_cache() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
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
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("retry returns metadata");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn repeated_unsolicited_304_reports_missing_cache() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("cache-control", Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("cache-control", "no-cache")
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
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("304 needs a cache");
    dbg!(&error);
    assert!(matches!(
        error,
        FetchMetadataError::NotModifiedWithoutCache { ref pkg_name } if pkg_name == "acme"
    ));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn full_metadata_cache_loss_after_304_retries_once_without_validators() {
    assert_cache_loss_after_304_recovers(true, FULL_META_DIR, true).await;
}

#[tokio::test]
async fn abbreviated_metadata_cache_loss_after_304_retries_once_without_validators() {
    assert_cache_loss_after_304_recovers(false, ABBREVIATED_META_DIR, false).await;
}

#[tokio::test]
async fn cache_loss_after_304_stops_after_one_fallback() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror_tolerant(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("fallback 304 fails");
    assert!(matches!(
        error,
        FetchMetadataError::NotModifiedWithoutCache { ref pkg_name } if pkg_name == "acme"
    ));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn cache_loss_after_304_body_retry_remains_bypassed() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror_tolerant(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let broken = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("content-encoding", "gzip")
        .with_body("this is not valid gzip")
        .expect(1)
        .create_async()
        .await;
    let recovered = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("etag", r#"W/"after-retry""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: false,
        retry_opts: fast_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("body retry succeeds");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    broken.assert_async().await;
    recovered.assert_async().await;
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    assert_eq!(persisted.name, "acme");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"after-retry""#));
}

#[tokio::test]
async fn cache_loss_after_304_registry_error_propagates() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror_tolerant(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let forbidden = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(403)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("403 propagates");
    assert!(matches!(
        error,
        FetchMetadataError::Network { ref error, .. }
            if error.status() == Some(reqwest::StatusCode::FORBIDDEN)
    ));
    first.assert_async().await;
    forbidden.assert_async().await;
}

async fn assert_cache_loss_after_304_recovers(
    full_metadata: bool,
    meta_dir: &str,
    scripts_expected: bool,
) {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), meta_dir, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror_tolerant(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let response_body = PACKAGE_BODY.replace(
        r#""dist": {"#,
        r#""scripts": { "postinstall": "echo cache-race-marker" }, "dist": {"#,
    );
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("etag", r#"W/"fresh""#)
        .with_body(response_body)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata,
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("fallback returns metadata");
    assert_eq!(pkg.name, "acme");
    let manifest = pkg.versions.get("1.0.0").expect("version");
    assert_eq!(manifest.other.contains_key("scripts"), scripts_expected);
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let persisted_manifest = persisted.versions.get("1.0.0").expect("persisted version");
    assert_eq!(persisted_manifest.other.contains_key("scripts"), scripts_expected);
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"fresh""#));
    first.assert_async().await;
    second.assert_async().await;
}

/// Removes the cache mirror at `path` if it exists.
///
/// This helper avoids panicking with a `NotFound` error if the file has already
/// been deleted (e.g. on a subsequent unexpected request hit to the mock server),
/// which would otherwise panic the mockito server thread and lead to process-aborting
/// lock poisoning (see issue #13105).
fn remove_raced_mirror_tolerant(path: &std::path::Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(_) if !path.exists() => {}
        Err(error) => panic!("remove raced mirror: {error}"),
    }
}

fn write_stale_mirror(
    cache_dir: &std::path::Path,
    meta_dir: &str,
    registry: &str,
) -> std::path::PathBuf {
    let mirror_path = get_pkg_mirror_path(cache_dir, meta_dir, registry, "acme").expect("path");
    let meta = serde_json::from_str(PACKAGE_BODY).expect("package body");
    save_meta_indexed(&mirror_path, &meta, Some(r#"W/"stale""#)).expect("write stale mirror");
    mirror_path
}

#[tokio::test]
async fn filtered_full_cache_writes_filtered_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let filtered_body = PACKAGE_BODY.replace(
        r#""dist": {"#,
        r#""readme": "drop me", "scripts": { "preinstall": "node build.js" }, "dist": {"#,
    );
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(filtered_body)
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
        filter_metadata: true,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 -> ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), FULL_FILTERED_META_DIR, &registry, "acme")
        .expect("filtered path");
    assert!(mirror_path.exists(), "filtered mirror file written");
    let unfiltered_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("full path");
    assert!(!unfiltered_path.exists(), "unfiltered mirror must not be written");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    assert!(!manifest.other.contains_key("readme"));
    assert!(!manifest.other.contains_key("scripts"));
}

const ACCEPT_ABBREVIATED: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

#[tokio::test]
async fn a_full_doc_served_for_an_abbreviated_request_is_normalized_before_caching() {
    let mut server = mockito::Server::new_async().await;
    // A full document: what a registry that ignores the abbreviated `Accept`
    // header (e.g. Azure DevOps Artifacts) serves. It carries per-version
    // fields the resolver never reads.
    let full_body = PACKAGE_BODY.replace(
        r#""dist": {"#,
        r#""readme": "drop me", "scripts": { "postinstall": "node install.js" }, "exports": { ".": "./index.js" }, "dependencies": { "bar": "^1.0.0" }, "dist": {"#,
    );
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        // application/json (not the abbreviated content type) signals that
        // the registry ignored the abbreviated `Accept` header and served
        // the full document.
        .with_header("content-type", "application/json")
        .with_body(full_body)
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
        full_metadata: false,
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("abbreviated path");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    // Install-irrelevant fields dropped.
    assert!(!manifest.other.contains_key("readme"));
    assert!(!manifest.other.contains_key("scripts"));
    assert!(!manifest.other.contains_key("exports"));
    // Install-relevant fields kept, so resolution is unchanged.
    assert_eq!(
        manifest.dependencies.as_ref().and_then(|deps| deps.get("bar")).map(String::as_str),
        Some("^1.0.0"),
    );
}

#[tokio::test]
async fn a_doc_served_with_the_abbreviated_content_type_is_cached_verbatim() {
    let mut server = mockito::Server::new_async().await;
    // A custom per-version field proves the fragment is mirrored verbatim
    // (no stripping) on the honored-header happy path.
    let abbreviated_body =
        PACKAGE_BODY.replace(r#""dist": {"#, r#""_cacheUntouchedMarker": "kept", "dist": {"#);
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        // Uppercase + a parameter: media-type detection must be
        // case-insensitive and drop parameters.
        .with_header("content-type", "APPLICATION/VND.NPM.INSTALL-V1+JSON; charset=utf-8")
        .with_body(abbreviated_body)
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
        full_metadata: false,
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("abbreviated path");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    assert!(manifest.other.contains_key("_cacheUntouchedMarker"));
}

#[tokio::test]
async fn warm_cache_serves_from_mirror_on_304() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
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
        filter_metadata: false,
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

#[tokio::test]
async fn a_304_renews_the_mirror_mtime() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    server
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
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    fetch_full_metadata_cached("acme", &opts).await.expect("200 populates cache");
    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");

    // Age the mirror far past any maturity cutoff.
    let aged = std::time::SystemTime::now() - std::time::Duration::from_hours(365 * 24);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&mirror_path)
        .expect("open mirror")
        .set_modified(aged)
        .expect("age mirror");

    fetch_full_metadata_cached("acme", &opts).await.expect("304 reads from mirror");

    let renewed = std::fs::metadata(&mirror_path).expect("stat mirror").modified().expect("mtime");
    let age = std::time::SystemTime::now().duration_since(renewed).expect("mtime in the past");
    assert!(
        age < std::time::Duration::from_mins(1),
        "mirror mtime must be renewed by the 304; still {age:?} old",
    );
}

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
        filter_metadata: false,
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
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;
}

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
        filter_metadata: false,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("read-only must not fail");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    // Restore so TempDir's drop can clean up.
    let _ = fs::set_permissions(cache.path(), fs::Permissions::from_mode(mode));
}

#[tokio::test]
async fn body_read_failure_retries_and_writes_mirror() {
    let mut server = mockito::Server::new_async().await;
    let first = corrupt_gzip_body_mock(&mut server).await;
    let second = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"after-retry""#)
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
        filter_metadata: false,
        retry_opts: fast_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("body read retries");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    second.assert_async().await;

    // The retry's body is what gets persisted to the mirror.
    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"after-retry""#));

    // A follow-up conditional GET answered 304 proves the persisted body is
    // a usable mirror, not just freshened headers over a missing/stale body.
    let not_modified = server.mock("GET", "/acme").with_status(304).expect(1).create_async().await;
    let cached_pkg =
        fetch_full_metadata_cached("acme", &opts).await.expect("mirror body readable after retry");
    assert_eq!(cached_pkg.name, "acme");
    not_modified.assert_async().await;
}
