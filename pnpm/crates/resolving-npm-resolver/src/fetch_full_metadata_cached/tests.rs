mod behavior;

mod metadata_cache_body_read_failure_retries;

mod metadata_cache_cold_cache_writes_mirror;

use mockito::Matcher;
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
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
            remove_raced_mirror(&raced_mirror);
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
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
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

fn remove_raced_mirror(mirror_path: &std::path::Path) {
    match std::fs::remove_file(mirror_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove raced mirror at {}: {error}", mirror_path.display()),
    }
}

const ACCEPT_ABBREVIATED: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";
