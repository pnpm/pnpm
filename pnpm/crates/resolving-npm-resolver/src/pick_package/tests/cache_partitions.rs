use super::{
    ABBREVIATED_BODY, ABBREVIATED_META_DIR, Arc, AuthHeaders, InMemoryPackageMetaCache,
    MetadataCacheScope, PACKAGE_BODY, PickPackageContext, PickPackageOptions, RetryOpts,
    RouteRecorder, ScopeHook, TempDir, ThrottledClient, UpstreamRouteHook, assert_eq, default_opts,
    get_pkg_mirror_path, metadata_cache_key, persist_meta_to_mirror, pick_package, range_spec,
    shared_packument_fetch_locker, to_registry_url, version_spec,
};
use crate::pick_package::metadata_cache::PackageMetaCache;

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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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

/// Every fast path that answers a pick from a warm in-memory or on-disk
/// cache must still record the route through the hook, so a server's
/// private-footprint stays complete regardless of cache state. Without
/// the up-front [`AuthHeaders::record_route`] in [`pick_package`], a
/// private package served from cache would never be recorded and its
/// resolution would be wrongly cached as public.
#[tokio::test]
async fn cache_fast_paths_record_route_through_hook() {
    let preloaded: pnpm_registry::Package =
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
            needs_full_metadata_for: None,
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
            needs_full_metadata_for: None,
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
            needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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

/// A public route keeps its disk fallback on the same `401`, proving the
/// fail-closed behavior is scoped to private routes only.
#[tokio::test]
async fn public_scope_falls_back_to_mirror_on_401() {
    let preloaded: pnpm_registry::Package =
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
