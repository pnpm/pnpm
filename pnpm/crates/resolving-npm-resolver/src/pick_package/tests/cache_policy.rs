//! What the three [`pnpm_config::NetworkPolicy`] states decide between,
//! told apart by whether the registry is contacted at all. The `offline` and
//! `preferOffline` settings reach the picker only through that policy.

use super::{
    ABBREVIATED_META_DIR, AuthHeaders, InMemoryPackageMetaCache, PACKAGE_BODY, PickPackageContext,
    PickPackageError, PickPackageOptions, RetryOpts, TempDir, ThrottledClient, assert_eq,
    default_opts, get_pkg_mirror_path, persist_meta_to_mirror, pick_package, range_spec,
    shared_packument_fetch_locker,
};

/// A warm mirror alone does not force a fetch: an online pick may return a
/// range's stable mirror pick unverified. `include_latest_tag` is one of the
/// conditions that withdraws that shortcut, and then only the registry can
/// answer. This test and the next differ only in the policy, `Online`
/// against `PreferCache`.
/// The pick reduced to a label, so a scenario table can assert on it.
fn outcome_label(result: Result<crate::PickPackageResult, PickPackageError>) -> String {
    match result {
        Ok(result) => result.picked_package
            .map(|picked| picked.version.to_string())
            .unwrap_or_else(|| "none".to_string()),
        Err(PickPackageError::NoOfflineMeta { .. }) => "no-offline-meta".to_string(),
        Err(error) => format!("{error:?}"),
    }
}

#[tokio::test]
async fn an_online_pick_verifies_an_unstable_mirror_pick_against_the_registry() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::Online,
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };
    let opts = PickPackageOptions { include_latest_tag: true, ..default_opts(&registry) };

    let result =
        pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("the pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_offline_accepts_the_mirror_pick_the_registry_would_have_verified() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::PreferCache,
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };
    let opts = PickPackageOptions { include_latest_tag: true, ..default_opts(&registry) };

    let result =
        pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("the mirror answers");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn prefer_offline_answers_a_plain_range_from_the_mirror() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::PreferCache,
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("the mirror answers");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn offline_answers_from_the_mirror_without_the_registry() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::Offline,
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    let result = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("the mirror answers");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn offline_without_a_mirror_surfaces_no_offline_meta() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::Offline,
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    let error = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("an empty mirror has nothing to answer with");
    assert!(
        matches!(error, PickPackageError::NoOfflineMeta { .. }),
        "expected NoOfflineMeta, got {error:?}",
    );
    mock.assert_async().await;
}

/// The `preferOffline` setting asks which source to try first. `offline`
/// removes the registry from the sources altogether, so once it is set the
/// first question has nothing left to decide: both settings resolve to
/// `Offline` and must agree on the answer and on the failure. A warm mirror
/// resolves the same way, an empty one fails the same way, and the registry
/// stays untouched throughout.
#[tokio::test]
async fn prefer_offline_changes_nothing_once_offline_forbids_the_registry() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let warm = TempDir::new().expect("tempdir");
    let empty = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(warm.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let fetch_locker = shared_packument_fetch_locker();
    let spec = range_spec("acme", "^1.0.0");
    let opts = default_opts(&registry);
    let mut outcomes = Vec::new();
    for (prefer_offline, cache_dir) in
        [(false, warm.path()), (true, warm.path()), (false, empty.path()), (true, empty.path())]
    {
        // A cache per pick, so the mirror decides each one rather than an
        // earlier pick's in-memory entry.
        let meta_cache = InMemoryPackageMetaCache::default();
        let ctx = PickPackageContext {
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            cache_policy: crate::MetadataCachePolicy {
                network: pnpm_config::NetworkPolicy::from_settings(true, prefer_offline),
                ignore_missing_time_field: false,
            },
            store_view: None,
            metadata: crate::MetadataRequestContext {
                meta_cache: &meta_cache,
                fetch_locker: &fetch_locker,
                cache_dir: Some(cache_dir),
                http: crate::MetadataHttpClient {
                    http_client: &http_client,
                    auth_headers: &auth_headers,
                    retry_opts: RetryOpts::default(),
                },
            },
        };
        outcomes.push(outcome_label(pick_package(&ctx, &spec, &opts).await));
    }

    assert_eq!(outcomes, ["1.1.0", "1.1.0", "no-offline-meta", "no-offline-meta"]);
    mock.assert_async().await;
}

/// An in-memory entry that came from the mirror rather than the registry is
/// reusable without revalidation only for a pick whose answer fresh metadata
/// could not change. `include_latest_tag` is not such a pick, so this test
/// and the next differ in the policy alone and count the requests the
/// second pick makes.
#[tokio::test]
async fn prefer_offline_reuses_an_in_memory_entry_the_registry_never_confirmed() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let build_ctx = |prefer_offline| PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::from_settings(false, prefer_offline),
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    // A plain online range pick settles from the mirror, leaving an
    // in-memory entry the registry never confirmed.
    pick_package(&build_ctx(false), &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("the mirror settles the range");

    let opts = PickPackageOptions { include_latest_tag: true, ..default_opts(&registry) };
    let result = pick_package(&build_ctx(true), &range_spec("acme", "^1.0.0"), &opts)
        .await
        .expect("the pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

#[tokio::test]
async fn an_online_pick_revalidates_an_in_memory_entry_the_registry_never_confirmed() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let build_ctx = |prefer_offline| PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::from_settings(false, prefer_offline),
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    // A plain online range pick settles from the mirror, leaving an
    // in-memory entry the registry never confirmed.
    pick_package(&build_ctx(false), &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("the mirror settles the range");

    let opts = PickPackageOptions { include_latest_tag: true, ..default_opts(&registry) };
    let result = pick_package(&build_ctx(false), &range_spec("acme", "^1.0.0"), &opts)
        .await
        .expect("the pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}

/// With the mirror gone, the in-memory entry it left behind is the only
/// offline source, and reusing it without revalidation is what
/// `PreferCache` buys. A store prune between two picks of one process
/// reaches this.
#[tokio::test]
async fn prefer_offline_reuses_a_cached_entry_whose_mirror_has_been_pruned() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(0)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = TempDir::new().expect("tempdir");
    let mirror = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("mirror path");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let build_ctx = |prefer_offline| PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            network: pnpm_config::NetworkPolicy::from_settings(false, prefer_offline),
            ignore_missing_time_field: false,
        },
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    pick_package(&build_ctx(false), &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect("the mirror settles the range");
    std::fs::remove_file(&mirror).expect("prune the mirror");

    let opts = PickPackageOptions { include_latest_tag: true, ..default_opts(&registry) };
    let result = pick_package(&build_ctx(true), &range_spec("acme", "^1.0.0"), &opts)
        .await
        .expect("the cached entry answers");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    mock.assert_async().await;
}
