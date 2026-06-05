use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use chrono::{DateTime, Utc};
use pacquet_config::version_policy::create_package_version_policy;

use super::{
    InMemoryPackageMetaCache, PackageMetaCache, PickPackageContext, PickPackageError,
    PickPackageOptions, persist_meta_to_mirror, pick_package, shared_packument_fetch_locker,
};
use crate::{
    mirror::{ABBREVIATED_META_DIR, FULL_META_DIR, get_pkg_mirror_path, load_meta},
    pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType},
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
        retry_opts: RetryOpts::default(),
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");

    let picked = result.picked_package.expect("picked something");
    assert_eq!(picked.version.to_string(), "1.1.0");
    mock.assert_async().await;

    // In-memory cache populated for the next call. Key is
    // registry-scoped (`<registry>\x00<name>`) so two registries
    // can't contaminate each other; we just check that *some* key
    // landed.
    let key = format!("{registry}\x00acme");
    assert!(meta_cache.get(&key).is_some(), "in-memory cache populated");
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
    let fetch_locker = shared_packument_fetch_locker();

    let preloaded: pacquet_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    // Cache key is `<registry>\x00<name>` — pre-seed at the same
    // key the orchestrator will look up on the first call.
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
        retry_opts: RetryOpts::default(),
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
        retry_opts: RetryOpts::default(),
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
        retry_opts: RetryOpts::default(),
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
        retry_opts: RetryOpts::default(),
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
        retry_opts: RetryOpts::default(),
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
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.dry_run = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    let key = format!("{registry}\x00acme");
    assert!(meta_cache.get(&key).is_none(), "dry_run must not poison the in-memory cache");
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
/// at the second registry. Mirrors upstream's per-resolver-instance
/// cache scoping.
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

/// Invalid package name (unscoped + slash) surfaces
/// `ERR_PNPM_INVALID_PACKAGE_NAME` before any IO runs.
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

/// Default-mode pick (`full_metadata=false`, no opts.optional) hits
/// the abbreviated install-v1 endpoint and caches under
/// `ABBREVIATED_META_DIR`. The full mirror stays untouched.
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

/// `opts.optional = true` forces full metadata even when
/// `ctx.full_metadata` is off. Mirrors upstream's
/// `fullMetadata = opts.optional || ctx.fullMetadata` derivation
/// (pnpm/pnpm#9950).
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

/// Two pick calls with different full-mode flags must not share an
/// in-memory cache slot — the abbreviated entry is missing fields
/// that a full-mode caller (optional dep) depends on. Mirrors
/// upstream's `cacheKey = fullMetadata ? '${name}:full' : name`
/// scoping.
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
        retry_opts: RetryOpts::default(),
    };

    // First call: default (abbreviated).
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("first");
    // Second call: optional=true (full). Cache key has `:full`
    // suffix so it must NOT hit the abbreviated slot — the
    // network mock for full must fire.
    let mut opts = default_opts(&registry);
    opts.optional = true;
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("second");

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
}

/// `published_by` active + abbreviated cache lacking `time` +
/// `modified` after the cutoff → re-fetch full metadata so the
/// maturity check runs on real timestamps. Persisting the upgrade
/// to the abbreviated mirror means the next call sees `time`
/// populated and skips the upgrade fetch entirely. Ports the spirit
/// of upstream's
/// [`upgrades cached abbreviated metadata to full when 304 Not Modified and publishedBy is set`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/test/publishedBy.test.ts#L450-L511).
#[tokio::test]
async fn published_by_triggers_upgrade_when_modified_after_cutoff() {
    let mut server = mockito::Server::new_async().await;
    // First fetch: abbreviated response (no `time`), recent
    // `modified` so the upgrade trigger fires.
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
    // Second fetch: the upgrade-to-full request. The body carries
    // a `time` map so the picker can run the maturity check.
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

    // The upgraded full meta is written back to the abbreviated
    // mirror so the next install sees `time` populated and skips
    // the upgrade fetch — matches upstream's `persistUpgradedMeta`.
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
/// the upgrade fetch is unnecessary. Mirrors upstream's strict
/// inclusive boundary in
/// [`maybeUpgradeAbbreviatedMetaForReleaseAge`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L474).
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
    // Fresh network metadata has 1.1.0 as latest.
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

/// Concurrent `pick_package` calls for the same `(registry, name)`
/// coalesce into a single network fetch. Mirrors pnpm's
/// [`runLimited(pkgMirror, …)`](https://github.com/pnpm/pnpm/blob/f657b5cb44/resolving/npm-resolver/src/pickPackage.ts#L52-L64)
/// behavior — without dedup, each duplicate caller would race past
/// the in-memory cache miss and fire its own GET, exhausting the
/// [`ThrottledClient`] semaphore and re-fetching the same packument
/// `N` times.
///
/// The mock asserts `expect(1)` — even though we spawn 20 concurrent
/// picks, exactly one GET reaches the registry. The other 19 wait
/// on the per-key permit and pick up the cached packument once the
/// winner returns.
#[tokio::test]
async fn concurrent_picks_for_same_key_share_one_network_fetch() {
    let mut server = mockito::Server::new_async().await;
    // `expect(1)` is the assertion: at most one GET reaches the
    // registry for the 20-way concurrent fan-out below. Without the
    // per-key serializer, all 20 would race past the empty in-memory
    // cache and each fire its own GET.
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
