//! What `offline` and `prefer_offline` decide between, told apart by
//! whether the registry is contacted at all.

use super::{
    ABBREVIATED_META_DIR, AuthHeaders, InMemoryPackageMetaCache, PACKAGE_BODY, PickPackageContext,
    PickPackageError, PickPackageOptions, RetryOpts, TempDir, ThrottledClient, assert_eq,
    default_opts, persist_meta_to_mirror, pick_package, range_spec, shared_packument_fetch_locker,
};

/// A warm mirror alone does not force a fetch: an online pick may return a
/// range's stable mirror pick unverified. `include_latest_tag` is one of the
/// conditions that withdraws that shortcut, and then only the registry can
/// answer. This test and the next differ in `prefer_offline` alone.
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
            offline: false,
            prefer_offline: false,
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
            offline: false,
            prefer_offline: true,
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
            offline: false,
            prefer_offline: true,
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
            offline: true,
            prefer_offline: false,
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
            offline: true,
            prefer_offline: false,
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

/// `prefer_offline` asks which source to try first. `offline` removes the
/// registry from the sources altogether, so once it is set the first
/// question has nothing left to decide: both settings of `prefer_offline`
/// resolve the same way, from the same mirror, with the registry untouched.
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
    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm the mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let fetch_locker = shared_packument_fetch_locker();
    let mut picked = Vec::new();
    for prefer_offline in [false, true] {
        // A cache per pick, so the mirror answers both rather than the
        // first pick's in-memory entry answering the second.
        let meta_cache = InMemoryPackageMetaCache::default();
        let ctx = PickPackageContext {
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            cache_policy: crate::MetadataCachePolicy {
                offline: true,
                prefer_offline,
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
        picked.push(result.picked_package.expect("picked").version.to_string());
    }

    assert_eq!(picked, ["1.1.0", "1.1.0"]);
    mock.assert_async().await;
}
