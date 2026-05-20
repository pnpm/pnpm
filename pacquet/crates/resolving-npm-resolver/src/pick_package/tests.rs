use pacquet_network::{AuthHeaders, ThrottledClient};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::{
    InMemoryPackageMetaCache, PackageMetaCache, PickPackageContext, PickPackageError,
    PickPackageOptions, persist_meta_to_mirror, pick_package,
};
use crate::pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType};

const PACKAGE_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.1.0" },
    "modified": "2025-01-15T12:00:00.000Z",
    "time": {
        "1.0.0": "2024-01-10T08:30:00.000Z",
        "1.1.0": "2024-12-10T08:30:00.000Z"
    },
    "versions": {
        "1.0.0": {
            "name": "acme",
            "version": "1.0.0",
            "dist": {
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                "shasum": "0000000000000000000000000000000000000000",
                "tarball": "https://registry/acme-1.0.0.tgz"
            }
        },
        "1.1.0": {
            "name": "acme",
            "version": "1.1.0",
            "dist": {
                "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                "shasum": "1111111111111111111111111111111111111111",
                "tarball": "https://registry/acme-1.1.0.tgz"
            }
        }
    }
}"#;

fn range_spec(name: &str, range: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: name.to_string(),
        fetch_spec: range.to_string(),
        spec_type: RegistryPackageSpecType::Range,
        normalized_bare_specifier: None,
    }
}

fn version_spec(name: &str, version: &str) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: name.to_string(),
        fetch_spec: version.to_string(),
        spec_type: RegistryPackageSpecType::Version,
        normalized_bare_specifier: None,
    }
}

fn default_opts<'a>(registry: &'a str) -> PickPackageOptions<'a> {
    PickPackageOptions {
        registry,
        preferred_version_selectors: None,
        published_by: None,
        published_by_exclude: None,
        pick_lowest_version: false,
        include_latest_tag: false,
        dry_run: false,
    }
}

/// Cold-cache pick fetches the registry, populates the in-memory
/// cache, and returns the max satisfying version.
#[tokio::test]
async fn cold_pick_fetches_and_picks_max_in_range() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");

    let picked = result.picked_package.expect("picked something");
    assert_eq!(picked.version.to_string(), "1.1.0");
    mock.assert_async().await;

    // In-memory cache populated for the next call.
    assert!(meta_cache.get("acme").is_some(), "in-memory cache populated");
}

/// Warm in-memory cache: no network call, picker reads the cached
/// packument directly.
#[tokio::test]
async fn warm_in_memory_cache_skips_network() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();

    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    meta_cache.set("acme".to_string(), preloaded);

    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

/// `offline=true` with a populated mirror reads the disk cache and
/// never hits the network.
#[tokio::test]
async fn offline_with_mirror_picks_from_disk() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), &registry, &preloaded).expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: true,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

/// `offline=true` with no mirror present surfaces
/// `ERR_PNPM_NO_OFFLINE_META`. Matches upstream's hard error at
/// pickPackage.ts#L242.
#[tokio::test]
async fn offline_without_mirror_errors() {
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = "https://registry.example.com/".to_string();
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: true,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let err = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("offline + no mirror = error");
    assert!(matches!(err, PickPackageError::NoOfflineMeta { .. }), "got {err:?}");
}

/// A pinned-version spec with an on-disk mirror that already has
/// that exact version takes the fast path: no network call.
#[tokio::test]
async fn version_spec_with_mirror_takes_fast_path() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), &registry, &preloaded).expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let result = pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

/// A pinned-version spec NOT present in the mirror falls through
/// to the network fetch.
#[tokio::test]
async fn version_spec_missing_in_mirror_fetches() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());

    // Seed the mirror with versions that don't include the
    // requested pin so the fast path declines and the network
    // fetch runs.
    let older_body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "0.9.0" },
        "modified": "2024-01-01T00:00:00.000Z",
        "time": {},
        "versions": {
            "0.9.0": {
                "name": "acme",
                "version": "0.9.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry/acme-0.9.0.tgz"
                }
            }
        }
    }"#;
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(older_body).expect("parse old packument");
    persist_meta_to_mirror(cache_dir.path(), &registry, &preloaded).expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let result = pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

/// `dry_run=true` does not populate the in-memory cache (so a
/// follow-up resolution sees a clean slate). The disk mirror still
/// gets written by the underlying fetcher — that divergence from
/// upstream is documented at the gating branch.
#[tokio::test]
async fn dry_run_skips_in_memory_cache() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let mut opts = default_opts(&registry);
    opts.dry_run = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    assert!(meta_cache.get("acme").is_none(), "dry_run must not poison the in-memory cache");
}

/// `pick_lowest_version=true` picks the min satisfying version.
#[tokio::test]
async fn pick_lowest_version_picks_min() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let mut opts = default_opts(&registry);
    opts.pick_lowest_version = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
}

/// Invalid package name (unscoped + slash) surfaces
/// `ERR_PNPM_INVALID_PACKAGE_NAME` before any IO runs.
#[tokio::test]
async fn invalid_package_name_errors_synchronously() {
    let registry = "https://registry.example.com/".to_string();
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        cache_dir: None,
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
    };

    let err = pick_package(&ctx, &range_spec("foo/bar", "*"), &default_opts(&registry))
        .await
        .expect_err("invalid name");
    assert!(matches!(err, PickPackageError::InvalidPackageName { .. }), "got {err:?}");
}
