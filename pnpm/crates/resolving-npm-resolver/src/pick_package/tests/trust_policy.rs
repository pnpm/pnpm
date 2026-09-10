use super::{
    ABBREVIATED_META_DIR, AuthHeaders, EXISTING_VERSION_SELECTOR_WEIGHT, InMemoryPackageMetaCache,
    PACKAGE_BODY, PickPackageContext, RetryOpts, TempDir, ThrottledClient, VersionSelectorEntry,
    VersionSelectorType, VersionSelectorWithWeight, VersionSelectors, default_opts,
    persist_meta_to_mirror, pick_package, range_spec, shared_packument_fetch_locker,
};

#[tokio::test]
async fn normal_range_fetches_when_trust_policy_is_active() {
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
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(&selectors);
    opts.trust_policy = Some(pnpm_config::TrustPolicy::NoDowngrade);

    pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    mock.assert_async().await;
}
