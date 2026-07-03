use std::sync::Arc;

use pacquet_network::{
    AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient, UpstreamRouteHook,
};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use chrono::{DateTime, Utc};
use pacquet_config::version_policy::create_package_version_policy;

use super::{
    InMemoryPackageMetaCache, PackageMetaCache, PickPackageContext, PickPackageError,
    PickPackageOptions, metadata_cache_key, persist_meta_to_mirror, pick_package,
    shared_packument_fetch_locker,
};
use crate::{
    mirror::{
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, get_pkg_mirror_path, load_meta,
    },
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
    registry_url::to_registry_url,
};

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

fn default_opts(registry: &str) -> PickPackageOptions<'_> {
    PickPackageOptions {
        registry,
        preferred_version_selectors: None,
        published_by: None,
        published_by_exclude: None,
        pick_lowest_version: false,
        include_latest_tag: false,
        dry_run: false,
        optional: false,
        update_checksums: false,
        blocked_versions: None,
    }
}

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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");

    let picked = result.picked_package.expect("picked something");
    assert_eq!(picked.version.to_string(), "1.1.0");
    mock.assert_async().await;

    let key = format!("{registry}\x00acme");
    assert!(meta_cache.get(&key).is_some(), "in-memory cache populated");
}

#[tokio::test]
async fn filtered_full_metadata_reads_pnpm_jsonl_mirror_for_lowest_pick() {
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = "https://registry.example.test/";
    let mirror_path =
        get_pkg_mirror_path(cache_dir.path(), FULL_FILTERED_META_DIR, registry, "acme")
            .expect("path");
    std::fs::create_dir_all(mirror_path.parent().expect("mirror parent")).expect("mkdir");
    std::fs::write(&mirror_path, format!("{{\"etag\":\"W/filtered\"}}\n{PACKAGE_BODY}"))
        .expect("write pnpm jsonl mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: true,
        filter_metadata: true,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(registry);
    opts.pick_lowest_version = true;

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
}

#[tokio::test]
async fn warm_in_memory_cache_skips_network() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();

    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    meta_cache.set(format!("{registry}\x00acme"), std::sync::Arc::new(preloaded));

    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn offline_with_mirror_picks_from_disk() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: true,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn offline_without_mirror_errors() {
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = "https://registry.example.com/".to_string();
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: true,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let err = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("offline + no mirror = error");
    assert!(matches!(err, PickPackageError::NoOfflineMeta { .. }), "got {err:?}");
}

#[tokio::test]
async fn version_spec_with_mirror_takes_fast_path() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

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
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.dry_run = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    let key = format!("{registry}\x00acme");
    assert!(meta_cache.get(&key).is_none(), "dry_run must not poison the in-memory cache");
}

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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.pick_lowest_version = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
}

/// The in-memory cache must be keyed by `(registry, name)`, not by
/// name alone — otherwise a packument fetched from one registry
/// would satisfy a later resolve against a different registry, and
/// the second resolve could return a version that doesn't exist
/// at the second registry.
#[tokio::test]
async fn in_memory_cache_does_not_leak_across_registries() {
    let mut server_a = mockito::Server::new_async().await;
    let mut server_b = mockito::Server::new_async().await;

    let body_a = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2024-01-01T00:00:00.000Z",
        "time": { "1.0.0": "2024-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "acme", "version": "1.0.0",
                "dist": {
                    "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "https://registry-a/acme-1.0.0.tgz"
                }
            }
        }
    }"#;
    let body_b = r#"{
        "name": "acme",
        "dist-tags": { "latest": "9.9.9" },
        "modified": "2024-01-01T00:00:00.000Z",
        "time": { "9.9.9": "2024-01-01T00:00:00.000Z" },
        "versions": {
            "9.9.9": {
                "name": "acme", "version": "9.9.9",
                "dist": {
                    "integrity": "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
                    "shasum": "1111111111111111111111111111111111111111",
                    "tarball": "https://registry-b/acme-9.9.9.tgz"
                }
            }
        }
    }"#;
    let mock_a = server_a
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_a)
        .expect(1)
        .create_async()
        .await;
    let mock_b = server_b
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(body_b)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry_a = format!("{}/", server_a.url());
    let registry_b = format!("{}/", server_b.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let pick_a = pick_package(&ctx, &range_spec("acme", "*"), &default_opts(&registry_a))
        .await
        .expect("a")
        .picked_package
        .expect("a picked");
    let pick_b = pick_package(&ctx, &range_spec("acme", "*"), &default_opts(&registry_b))
        .await
        .expect("b")
        .picked_package
        .expect("b picked");

    assert_eq!(pick_a.version.to_string(), "1.0.0", "registry A's packument wins for A");
    assert_eq!(
        pick_b.version.to_string(),
        "9.9.9",
        "registry B must NOT reuse A's cached packument",
    );
    mock_a.assert_async().await;
    mock_b.assert_async().await;
}

#[tokio::test]
async fn invalid_package_name_errors_synchronously() {
    let registry = "https://registry.example.com/".to_string();
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: None,
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let err = pick_package(&ctx, &range_spec("foo/bar", "*"), &default_opts(&registry))
        .await
        .expect_err("invalid name");
    assert!(matches!(err, PickPackageError::InvalidPackageName { .. }), "got {err:?}");
}

/// Abbreviated metadata body, mirroring what a real npm registry
/// returns under `Accept: application/vnd.npm.install-v1+json`.
/// Missing per-version `time`, no `_npmUser`, no `dist.attestations`
/// — just the picker-relevant shape.
const ABBREVIATED_BODY: &str = r#"{
    "name": "acme",
    "dist-tags": { "latest": "1.0.0" },
    "modified": "2024-12-01T00:00:00.000Z",
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

fn parse_cutoff(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339).expect("parse cutoff").with_timezone(&Utc)
}

#[tokio::test]
async fn default_pick_targets_abbreviated_endpoint_and_mirror() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;

    let abbrev_path =
        get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("path");
    assert!(abbrev_path.exists(), "abbreviated mirror written");
    let full_path =
        get_pkg_mirror_path(cache_dir.path(), FULL_META_DIR, &registry, "acme").expect("path");
    assert!(!full_path.exists(), "full mirror left untouched on default pick");
}

#[tokio::test]
async fn optional_opt_forces_full_metadata_endpoint() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.optional = true;
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    mock.assert_async().await;

    let full_path =
        get_pkg_mirror_path(cache_dir.path(), FULL_META_DIR, &registry, "acme").expect("path");
    assert!(full_path.exists(), "full mirror written when optional=true");
}

#[tokio::test]
async fn cache_key_separates_abbreviated_from_full() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("first");
    let mut opts = default_opts(&registry);
    opts.optional = true;
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("second");

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
}

#[tokio::test]
async fn cache_key_separates_filtered_full_from_unfiltered_full() {
    let mut server = mockito::Server::new_async().await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(2)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let unfiltered_ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: true,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let filtered_ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: true,
        filter_metadata: true,
        retry_opts: RetryOpts::default(),
    };

    let _ = pick_package(&unfiltered_ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("unfiltered full");
    let _ = pick_package(&filtered_ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("filtered full");

    assert!(meta_cache.get(&format!("{registry}\x00acme:full")).is_some());
    assert!(meta_cache.get(&format!("{registry}\x00acme:full:filtered")).is_some());
    full_mock.assert_async().await;
}

#[tokio::test]
async fn published_by_triggers_upgrade_when_modified_after_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    // Cutoff sits before `modified=2024-12-01` AND before every
    // version's publish date in PACKAGE_BODY, so every version is
    // immature; the picker still returns a fall-back pick so the
    // call doesn't error.
    opts.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;

    let abbrev_path =
        get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("path");
    let persisted = load_meta(&abbrev_path).expect("abbreviated mirror readable");
    assert!(
        persisted.time.is_some(),
        "abbreviated mirror should now carry time so the next install skips the upgrade",
    );
}

/// Boundary case: `modified == cutoff`. `modified` is an upper
/// bound on every version's publish time, so when it equals the
/// cutoff every version passes the per-version `<=` filter and
/// the upgrade fetch is unnecessary (the boundary is inclusive).
#[tokio::test]
async fn published_by_skips_upgrade_when_modified_equals_cutoff() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    // No second mock — the registry must NOT see a full-metadata
    // request. If the upgrade trigger fires by mistake, the test
    // panics on an unmatched request from `mockito`.

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: true,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.published_by = Some(parse_cutoff("2024-12-01T00:00:00Z"));
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    abbrev_mock.assert_async().await;
}

/// Excluded packages must skip abbreviated->full upgrade even when
/// `modified` is newer than the cutoff, because minimumReleaseAge is
/// disabled for `PolicyMatch::AnyVersion`.
#[tokio::test]
async fn published_by_exclude_skips_upgrade_for_abbreviated_meta_without_time() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;

    // No full-metadata mock on purpose: excluded packages must not trigger
    // the upgrade fetch even when abbreviated metadata has no `time` field.
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let policy = create_package_version_policy(["acme"]).expect("policy");
    let mut opts = default_opts(&registry);
    opts.published_by = Some(parse_cutoff("2020-01-01T00:00:00Z"));
    opts.published_by_exclude = Some(&policy);

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(
        result.picked_package.expect("picked").version.to_string(),
        "1.0.0",
        "exclude policy should bypass release-age upgrade and pick from abbreviated meta",
    );
    abbrev_mock.assert_async().await;
}

/// Fully excluded packages (`minimumReleaseAgeExclude: ['acme']`) must bypass
/// the publishedBy file-mtime cache shortcut, otherwise a stale abbreviated
/// mirror can pin resolution to an old latest forever until the cutoff window
/// moves past the file mtime.
#[tokio::test]
async fn published_by_excluded_package_bypasses_mtime_shortcut_and_revalidates() {
    let mut server = mockito::Server::new_async().await;
    let network_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());

    // Stale abbreviated mirror missing 1.1.0 entirely.
    let stale_body = r#"{
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "modified": "2024-01-01T00:00:00.000Z",
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
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(stale_body).expect("parse stale packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm stale mirror");
    let mirror_path =
        get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("path");
    let forced_mtime: std::time::SystemTime = parse_cutoff("2024-01-01T00:00:00Z").into();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&mirror_path)
        .expect("open stale mirror")
        .set_times(std::fs::FileTimes::new().set_modified(forced_mtime))
        .expect("set stale mirror mtime");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let policy = create_package_version_policy(["acme"]).expect("policy");
    let mut opts = default_opts(&registry);
    // Keep the mtime-guard condition deterministic: mirror mtime is set
    // explicitly to 2024-01-01 above.
    opts.published_by = Some(parse_cutoff("2020-01-01T00:00:00Z"));
    opts.published_by_exclude = Some(&policy);

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(
        result.picked_package.expect("picked").version.to_string(),
        "1.1.0",
        "excluded package should revalidate stale mirror and pick fresh latest",
    );
    network_mock.assert_async().await;
}

#[tokio::test]
async fn concurrent_picks_for_same_key_share_one_network_fetch() {
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
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let spec = range_spec("acme", "^1.0.0");
    let opts = default_opts(&registry);
    let results =
        futures_util::future::try_join_all((0..20).map(|_| pick_package(&ctx, &spec, &opts)))
            .await
            .expect("all picks succeed");

    for result in results {
        assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn update_checksums_bypasses_warm_in_memory_cache() {
    let mut server = mockito::Server::new_async().await;
    // Only the update_checksums revalidation should hit the network; the first,
    // non-update_checksums pick is served from the on-disk mirror.
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    // Normal pick takes the on-disk fast path and promotes the packument into
    // the in-memory cache.
    let first = pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(first.picked_package.expect("picked").version.to_string(), "1.0.0");
    assert!(meta_cache.get(&format!("{registry}\x00acme")).is_some(), "in-memory cache populated");

    // update_checksums must still revalidate against the registry despite the
    // warm in-memory cache holding a disk-sourced entry.
    let update_opts = PickPackageOptions { update_checksums: true, ..default_opts(&registry) };
    let second =
        pick_package(&ctx, &version_spec("acme", "1.0.0"), &update_opts).await.expect("ok");
    assert_eq!(second.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

/// A route hook that records every `(url, package)` it is asked to
/// classify, so a test can assert a cache-served pick still surfaces its
/// route to a server footprint even though no HTTP request happened.
#[derive(Default)]
struct RouteRecorder {
    routes: std::sync::Mutex<Vec<(String, Option<String>)>>,
}

impl UpstreamRouteHook for RouteRecorder {
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String> {
        self.routes
            .lock()
            .expect("route recorder poisoned")
            .push((url.to_string(), package.map(str::to_string)));
        None
    }
}

/// Every fast path that answers a pick from a warm in-memory or on-disk
/// cache must still record the route through the hook, so a server's
/// private-footprint stays complete regardless of cache state. Without
/// the up-front [`AuthHeaders::record_route`] in [`pick_package`], a
/// private package served from cache would never be recorded and its
/// resolution would be wrongly cached as public.
#[tokio::test]
async fn cache_fast_paths_record_route_through_hook() {
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");

    // The mock 500s and expects zero calls: every pick below is served
    // from cache, so the only signal the route was seen is the recorder.
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;
    let registry = format!("{}/", server.url());
    let expected_url = to_registry_url(&registry, "acme");

    // In-memory cache hit.
    {
        let recorder = Arc::new(RouteRecorder::default());
        let auth_headers = AuthHeaders::default()
            .with_route_hook(Arc::clone(&recorder) as Arc<dyn UpstreamRouteHook>);
        let cache_dir = TempDir::new().expect("tempdir");
        let http_client = ThrottledClient::default();
        let meta_cache = InMemoryPackageMetaCache::default();
        meta_cache.set(format!("{registry}\x00acme"), Arc::new(preloaded.clone()));
        let fetch_locker = shared_packument_fetch_locker();
        let ctx = PickPackageContext {
            http_client: &http_client,
            auth_headers: &auth_headers,
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            offline: false,
            prefer_offline: false,
            ignore_missing_time_field: false,
            full_metadata: false,
            filter_metadata: false,
            retry_opts: RetryOpts::default(),
        };
        pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
            .await
            .expect("ok");
        let routes = recorder.routes.lock().expect("routes");
        assert_eq!(routes.as_slice(), &[(expected_url.clone(), Some("acme".to_string()))]);
    }

    // Offline disk fast path.
    {
        let recorder = Arc::new(RouteRecorder::default());
        let auth_headers = AuthHeaders::default()
            .with_route_hook(Arc::clone(&recorder) as Arc<dyn UpstreamRouteHook>);
        let cache_dir = TempDir::new().expect("tempdir");
        persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
            .expect("warm mirror");
        let http_client = ThrottledClient::default();
        let meta_cache = InMemoryPackageMetaCache::default();
        let fetch_locker = shared_packument_fetch_locker();
        let ctx = PickPackageContext {
            http_client: &http_client,
            auth_headers: &auth_headers,
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            offline: true,
            prefer_offline: false,
            ignore_missing_time_field: false,
            full_metadata: false,
            filter_metadata: false,
            retry_opts: RetryOpts::default(),
        };
        pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
            .await
            .expect("ok");
        let routes = recorder.routes.lock().expect("routes");
        assert_eq!(routes.as_slice(), &[(expected_url.clone(), Some("acme".to_string()))]);
    }

    // Version-spec disk fast path (no network, pinned version on disk).
    {
        let recorder = Arc::new(RouteRecorder::default());
        let auth_headers = AuthHeaders::default()
            .with_route_hook(Arc::clone(&recorder) as Arc<dyn UpstreamRouteHook>);
        let cache_dir = TempDir::new().expect("tempdir");
        persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
            .expect("warm mirror");
        let http_client = ThrottledClient::default();
        let meta_cache = InMemoryPackageMetaCache::default();
        let fetch_locker = shared_packument_fetch_locker();
        let ctx = PickPackageContext {
            http_client: &http_client,
            auth_headers: &auth_headers,
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            offline: false,
            prefer_offline: false,
            ignore_missing_time_field: false,
            full_metadata: false,
            filter_metadata: false,
            retry_opts: RetryOpts::default(),
        };
        pick_package(&ctx, &version_spec("acme", "1.0.0"), &default_opts(&registry))
            .await
            .expect("ok");
        let routes = recorder.routes.lock().expect("routes");
        assert_eq!(routes.as_slice(), &[(expected_url.clone(), Some("acme".to_string()))]);
    }

    mock.assert_async().await;
}

#[test]
fn metadata_cache_key_public_matches_upstream_shape() {
    // The CLI / public route must keep the exact `{registry}\x00{name}`
    // key (plus the `:full` suffixes) so behavior is unchanged.
    let scope = MetadataCacheScope::Public;
    assert_eq!(
        metadata_cache_key(&scope, "https://reg/", "acme", false, false),
        "https://reg/\x00acme",
    );
    assert_eq!(
        metadata_cache_key(&scope, "https://reg/", "acme", true, false),
        "https://reg/\x00acme:full",
    );
    assert_eq!(
        metadata_cache_key(&scope, "https://reg/", "acme", true, true),
        "https://reg/\x00acme:full:filtered",
    );
}

#[test]
fn metadata_cache_key_private_is_namespaced() {
    let private = MetadataCacheScope::Private { descriptor_id: "id1".to_string() };
    assert_eq!(
        metadata_cache_key(&private, "https://reg/", "acme", false, false),
        "private\x00id1\x00https://reg/\x00acme",
    );
    // A different descriptor never collides with the first.
    let other = MetadataCacheScope::Private { descriptor_id: "id2".to_string() };
    assert_ne!(
        metadata_cache_key(&private, "https://reg/", "acme", false, false),
        metadata_cache_key(&other, "https://reg/", "acme", false, false),
    );
    // A private key never collides with the public key.
    let public =
        metadata_cache_key(&MetadataCacheScope::Public, "https://reg/", "acme", false, false);
    assert_ne!(public, metadata_cache_key(&private, "https://reg/", "acme", false, false));
}

/// A route hook that classifies every fetch into one fixed
/// [`MetadataCacheScope`], so a test can drive the private mirror path
/// without standing up a full pnpr route policy.
struct ScopeHook {
    scope: MetadataCacheScope,
}

impl UpstreamRouteHook for ScopeHook {
    fn authorize(&self, _url: &str, _package: Option<&str>) -> Option<String> {
        None
    }

    fn metadata_scope(&self, _url: &str, _package: Option<&str>) -> MetadataCacheScope {
        self.scope.clone()
    }
}

/// A `Private` route must persist its packument under the
/// descriptor-namespaced mirror and never under the global one, so a
/// caller who can't reproduce the descriptor can't read it.
#[tokio::test]
async fn private_scope_writes_descriptor_namespaced_mirror() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default().with_route_hook(Arc::new(ScopeHook {
        scope: MetadataCacheScope::Private { descriptor_id: "deadbeef".to_string() },
    }) as Arc<dyn UpstreamRouteHook>);
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry)).await.expect("ok");
    mock.assert_async().await;

    let scoped = get_pkg_mirror_path(
        cache_dir.path(),
        "v11/metadata-private/deadbeef/metadata",
        &registry,
        "acme",
    )
    .expect("scoped path");
    assert!(scoped.exists(), "private packument lands in the descriptor-scoped mirror");
    let global = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("global path");
    assert!(!global.exists(), "private packument must not touch the global mirror");
}

/// A `Private` route must fail closed on a `401`: a revoked credential
/// must not keep serving the last cached packument, even from the route's
/// own descriptor-scoped mirror.
#[tokio::test]
async fn private_scope_fails_closed_on_401_without_disk_fallback() {
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(401).create_async().await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    // Warm the descriptor-scoped mirror that the fail-closed path must NOT
    // serve from.
    persist_meta_to_mirror(
        cache_dir.path(),
        "v11/metadata-private/deadbeef/metadata",
        &registry,
        &preloaded,
    )
    .expect("warm scoped mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default().with_route_hook(Arc::new(ScopeHook {
        scope: MetadataCacheScope::Private { descriptor_id: "deadbeef".to_string() },
    }) as Arc<dyn UpstreamRouteHook>);
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry)).await;
    assert!(result.is_err(), "private 401 fails closed instead of serving the stale mirror");
    mock.assert_async().await;
}

/// A public route keeps its disk fallback on the same `401`, proving the
/// fail-closed behavior is scoped to private routes only.
#[tokio::test]
async fn public_scope_falls_back_to_mirror_on_401() {
    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(401).create_async().await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm global mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}
