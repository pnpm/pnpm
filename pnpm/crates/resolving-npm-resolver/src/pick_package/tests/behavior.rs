use super::{
    Arc, AuthHeaders, InMemoryPackageMetaCache, MetadataCacheScope, PACKAGE_BODY,
    PickPackageContext, RetryOpts, ScopeHook, TempDir, ThrottledClient, UpstreamRouteHook,
    assert_eq, default_opts, persist_meta_to_mirror, pick_package, range_spec,
    shared_packument_fetch_locker,
};

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
        needs_full_metadata_for: None,
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

/// A `Private` route must fail closed on a `401`: a revoked credential
/// must not keep serving the last cached packument, even from the route's
/// own descriptor-scoped mirror.
#[tokio::test]
async fn private_scope_fails_closed_on_401_without_disk_fallback() {
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry)).await;
    assert!(result.is_err(), "private 401 fails closed instead of serving the stale mirror");
    mock.assert_async().await;
}
