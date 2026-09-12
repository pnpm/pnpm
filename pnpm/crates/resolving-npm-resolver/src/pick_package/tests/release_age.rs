use super::{
    ABBREVIATED_BODY, ABBREVIATED_META_DIR, Arc, AuthHeaders, InMemoryPackageMetaCache,
    PACKAGE_BODY, PARTIAL_TIME_PACKAGE_BODY, PickPackageContext, PickPackageError,
    PickPackageOptions, RetryOpts, TempDir, ThrottledClient, assert_eq,
    create_package_version_policy, default_opts, get_pkg_mirror_path, load_meta, parse_cutoff,
    persist_meta_to_mirror, pick_package, range_spec, shared_packument_fetch_locker,
};
use crate::pick_package::metadata_cache::PackageMetaCache;

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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let err = pick_package(&ctx, &range_spec("foo/bar", "*"), &default_opts(&registry))
        .await
        .expect_err("invalid name");
    assert!(matches!(err, PickPackageError::InvalidPackageName { .. }), "got {err:?}");
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
        needs_full_metadata_for: None,
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

/// A `time` map covering only some of the packument's versions can't decide
/// maturity for the rest: the untimed ones drop out of the filter and
/// resolution falls back to the lowest match. Upgrade to full metadata first,
/// so every version is judged on a real publish timestamp.
#[tokio::test]
async fn published_by_upgrades_metadata_with_partial_time_map() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
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

    let mut opts = default_opts(&registry);
    opts.published_by = Some(parse_cutoff("2025-01-01T00:00:00Z"));
    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("ok");

    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
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
        needs_full_metadata_for: None,
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
        needs_full_metadata_for: None,
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

/// A `304 Not Modified` answer to the release-age upgrade is remembered within
/// one install, but not by a metadata cache reused by the next install.
#[tokio::test]
async fn published_by_upgrade_not_modified_marker_is_scoped_to_install() {
    let mut server = mockito::Server::new_async().await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""acme-etag""#)
        .with_status(304)
        .expect(2)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    // The document a prior mirror load would have produced: abbreviated
    // (no `time`), carrying the mirror's etag as the upgrade validator.
    let mut seeded: pnpm_registry::Package =
        serde_json::from_str(ABBREVIATED_BODY).expect("parse fixture");
    seeded.etag = Some(r#""acme-etag""#.to_string());
    meta_cache.set(format!("{registry}\u{0}acme"), Arc::new(seeded));
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        // The post-304 document still has no `time`, so let the picker
        // take its warn-and-skip fallback instead of erroring.
        ignore_missing_time_field: true,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    // Cutoff before `modified=2024-12-01`, so the upgrade trigger fires.
    opts.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));

    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("first pick");
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("second pick");

    // A new install gets fresh fetch state but reuses the caller-owned metadata
    // cache. It must retry the upgrade instead of inheriting the first
    // install's marker from the cached Package.
    let next_install_fetch_locker = shared_packument_fetch_locker();
    let next_install_ctx = PickPackageContext {
        http_client: &http_client,
        auth_headers: &auth_headers,
        meta_cache: &meta_cache,
        fetch_locker: &next_install_fetch_locker,
        cache_dir: Some(cache_dir.path()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: true,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    let _ = pick_package(&next_install_ctx, &range_spec("acme", "^1.0.0"), &opts)
        .await
        .expect("next install pick");

    // One request for each install: the repeat pick in the first install is
    // the only one suppressed.
    full_mock.assert_async().await;
}

/// A registry whose full representation is no more complete than its
/// abbreviated one answers the upgrade with `200`, not `304`. That outcome
/// has to be remembered too, or every dependency edge re-asks for the same
/// full document and one package amplifies into a request per edge.
#[tokio::test]
async fn published_by_upgrade_answering_200_is_remembered_across_picks() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
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
        // The upgraded document is still undecidable, so let the picker take
        // its warn-and-skip fallback instead of erroring.
        ignore_missing_time_field: true,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));

    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("first pick");
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("second pick");

    // One abbreviated fetch and one upgrade, not one upgrade per pick.
    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
}

/// A same-install checksum refresh can replace an abbreviated packument while
/// retaining its cache key. A prior document's `304` marker must not suppress
/// the full-metadata check for that new response.
#[tokio::test]
async fn published_by_upgrade_not_modified_marker_is_scoped_to_document() {
    let mut server = mockito::Server::new_async().await;
    let first_full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""acme-etag""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let abbreviated_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_header("etag", r#""acme-etag-2""#)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let second_full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""acme-etag-2""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let mut seeded: pnpm_registry::Package =
        serde_json::from_str(ABBREVIATED_BODY).expect("parse fixture");
    seeded.etag = Some(r#""acme-etag""#.to_string());
    meta_cache.set(format!("{registry}\u{0}acme"), Arc::new(seeded));
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
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };

    let mut opts = default_opts(&registry);
    opts.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("first pick");

    let update_opts = PickPackageOptions { update_checksums: true, ..opts };
    let _ = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &update_opts)
        .await
        .expect("checksum-refresh pick");

    first_full_mock.assert_async().await;
    abbreviated_mock.assert_async().await;
    second_full_mock.assert_async().await;
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
    let preloaded: pnpm_registry::Package =
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
        needs_full_metadata_for: None,
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
