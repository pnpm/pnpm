use super::{
    ABBREVIATED_META_DIR, AuthHeaders, EXISTING_VERSION_SELECTOR_WEIGHT, InMemoryPackageMetaCache,
    PACKAGE_BODY, PickPackageContext, RetryOpts, STALE_PACKAGE_BODY, TempDir, ThrottledClient,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight, VersionSelectors,
    assert_eq, default_opts, persist_meta_to_mirror, pick_package, range_spec,
    shared_packument_fetch_locker,
};
use crate::pick_package::metadata_cache::PackageMetaCache;

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
        needs_full_metadata_for: None,
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
async fn normal_range_reuses_dominant_lockfile_version_from_disk() {
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
    let mut selectors = VersionSelectors::new();
    selectors.insert(
        "1.0.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(&selectors);

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn stable_range_does_not_promote_meta_for_a_later_unproven_range() {
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
        "1.0.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(&selectors);

    let first = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("first");
    let second = pick_package(&ctx, &range_spec("acme", ">=1.1.0"), &opts).await.expect("second");

    assert_eq!(first.picked_package.expect("first pick").version.to_string(), "1.0.0");
    assert_eq!(second.picked_package.expect("second pick").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn blocked_dominant_version_falls_through_to_registry_pick() {
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
    let cached: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &cached)
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
        "1.0.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );
    let blocked = std::iter::once("1.0.0".to_string()).collect();
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(&selectors);
    opts.blocked_versions = Some(&blocked);

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.pick_lowest_version = true;
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
}
