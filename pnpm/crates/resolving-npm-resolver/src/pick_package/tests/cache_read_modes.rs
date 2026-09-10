use super::{
    ABBREVIATED_BODY, ABBREVIATED_META_DIR, Arc, AuthHeaders, EXISTING_VERSION_SELECTOR_WEIGHT,
    FULL_FILTERED_META_DIR, FULL_META_DIR, InMemoryPackageMetaCache, MetadataCacheScope,
    PACKAGE_BODY, PickPackageContext, PickPackageError, RetryOpts, STALE_PACKAGE_BODY, TempDir,
    ThrottledClient, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
    VersionSelectors, assert_eq, default_opts, get_pkg_mirror_path, metadata_cache_key,
    persist_meta_to_mirror, pick_package, range_spec, shared_packument_fetch_locker, version_spec,
};
use crate::pick_package::metadata_cache::PackageMetaCache;

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
        needs_full_metadata_for: None,
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

    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
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
async fn normal_range_fetches_when_cached_meta_is_missing_lockfile_version() {
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
    let stale: pnpm_registry::Package =
        serde_json::from_str(STALE_PACKAGE_BODY).expect("parse stale packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &stale)
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let mut selectors = VersionSelectors::new();
    selectors.insert(
        "1.1.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(&selectors);

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn offline_with_mirror_picks_from_disk() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let err = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("offline + no mirror = error");
    assert!(matches!(err, PickPackageError::NoOfflineMeta { .. }), "got {err:?}");
}

/// The verification state lives inside the cache entry, so an
/// overwrite can never be observed with the previous entry's state.
#[test]
fn meta_cache_tracks_registry_verification_per_entry() {
    let cache = InMemoryPackageMetaCache::default();
    let meta: pnpm_registry::Package = serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let meta = Arc::new(meta);
    cache.set_unverified("k".to_string(), Arc::clone(&meta));
    assert!(!cache.get("k").expect("entry").registry_verified);
    cache.set("k".to_string(), meta);
    assert!(cache.get("k").expect("entry").registry_verified);
}

/// A second offline resolve succeeds after the mirror file is deleted,
/// proving the first resolve promoted the disk-loaded packument into
/// the in-memory cache instead of re-reading disk per dependent.
#[tokio::test]
async fn offline_promotes_disk_loaded_packument_into_memory_cache() {
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = "https://registry.example.com/".to_string();
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let spec = range_spec("acme", "^1.0.0");

    let first = pick_package(&ctx, &spec, &default_opts(&registry)).await.expect("ok");
    assert_eq!(first.picked_package.expect("picked").version.to_string(), "1.1.0");
    let cache_key =
        metadata_cache_key(&MetadataCacheScope::Public, &registry, "acme", false, false);
    assert!(meta_cache.get(&cache_key).is_some());

    let mirror = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("mirror path");
    std::fs::remove_file(mirror).expect("remove mirror");
    let second = pick_package(&ctx, &spec, &default_opts(&registry)).await.expect("ok");
    assert_eq!(second.picked_package.expect("picked").version.to_string(), "1.1.0");
}

/// Prefer-offline sibling of
/// [`offline_promotes_disk_loaded_packument_into_memory_cache`]; the
/// `expect(0)` mock additionally proves neither resolve touched the
/// registry.
#[tokio::test]
async fn prefer_offline_promotes_disk_loaded_packument_into_memory_cache() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pnpm_registry::Package =
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
        prefer_offline: true,
        ignore_missing_time_field: false,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let spec = range_spec("acme", "^1.0.0");

    let first = pick_package(&ctx, &spec, &default_opts(&registry)).await.expect("ok");
    assert_eq!(first.picked_package.expect("picked").version.to_string(), "1.1.0");
    let cache_key =
        metadata_cache_key(&MetadataCacheScope::Public, &registry, "acme", false, false);
    assert!(meta_cache.get(&cache_key).is_some());

    let mirror = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("mirror path");
    std::fs::remove_file(mirror).expect("remove mirror");
    let second = pick_package(&ctx, &spec, &default_opts(&registry)).await.expect("ok");
    assert_eq!(second.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

/// The first resolve picking 1.0.0 proves it was served from the stale
/// mirror (the registry would offer 1.1.0). The promoted entry can't
/// satisfy `^1.1.0`, so the second resolve must fall back to the
/// registry instead of failing the pick. The fetch replaces the entry
/// with a verified one, so the third resolve short-circuits on the
/// cache — `expect(1)` fails if it fetched again.
#[tokio::test]
async fn stale_disk_promoted_entry_falls_back_to_registry_under_prefer_offline() {
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
    let stale: pnpm_registry::Package =
        serde_json::from_str(STALE_PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &stale)
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
        prefer_offline: true,
        ignore_missing_time_field: false,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let first = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(first.picked_package.expect("picked").version.to_string(), "1.0.0");

    let second = pick_package(&ctx, &range_spec("acme", "^1.1.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(second.picked_package.expect("picked").version.to_string(), "1.1.0");

    let third = pick_package(&ctx, &range_spec("acme", "^1.1.0"), &default_opts(&registry))
        .await
        .expect("ok");
    assert_eq!(third.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn version_spec_with_mirror_takes_fast_path() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
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
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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
    let package_body = PACKAGE_BODY.replacen(
        r#""version": "1.0.0","#,
        r#""version": "1.0.0", "libc": ["glibc"],"#,
        1,
    );
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(package_body)
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.optional = true;
    let result = pick_package(&ctx, &version_spec("acme", "1.0.0"), &opts).await.expect("ok");
    mock.assert_async().await;

    assert_eq!(
        result.picked_package.expect("picked package").other.get("libc"),
        Some(&serde_json::json!(["glibc"])),
    );

    let full_path =
        get_pkg_mirror_path(cache_dir.path(), FULL_META_DIR, &registry, "acme").expect("path");
    assert!(full_path.exists(), "full mirror written when optional=true");
}
