use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pnpm_store_dir::{PackageFilesIndex, StoreIndex, store_index_key};

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::{
    AuthHeaders, InMemoryPackageMetaCache, PACKAGE_BODY, PickPackageContext, RetryOpts,
    ThrottledClient, default_opts, persist_meta_to_mirror, pick_package, range_spec,
    shared_packument_fetch_locker,
};
use crate::OfflineStoreView;
use crate::mirror::ABBREVIATED_META_DIR;

/// Offline mode resolves against the store: when the packument offers several
/// versions but the store holds only an older one, the pick must land on the
/// store-held version instead of the newest one, whose tarball the fetcher
/// could only reject with `ERR_PNPM_NO_OFFLINE_TARBALL`
/// ([pnpm/pnpm#10715](https://github.com/pnpm/pnpm/issues/10715)).
fn offline_ctx<'a>(
    cache_dir: &'a TempDir,
    store_view: Option<&'a OfflineStoreView>,
    meta_cache: &'a InMemoryPackageMetaCache,
    fetch_locker: &'a crate::PackumentFetchLocker,
    http_client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
) -> PickPackageContext<'a, InMemoryPackageMetaCache> {
    PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy {
            offline: true,
            prefer_offline: false,
            ignore_missing_time_field: false,
        },
        store_view,
        metadata: crate::MetadataRequestContext {
            meta_cache,
            fetch_locker,
            cache_dir: Some(cache_dir.path()),
            http: crate::MetadataHttpClient {
                http_client,
                auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    }
}

fn seed_store(store_dir: &TempDir, pkg_id: &str, integrity: &str) -> OfflineStoreView {
    let index = StoreIndex::open(store_dir.path()).expect("open store index for write");
    index
        .set(
            &store_index_key(integrity, pkg_id),
            &PackageFilesIndex {
                manifest: Some(serde_json::json!({
                    "name": "acme",
                    "version": "1.0.0",
                })),
                algo: "sha512".to_string(),
                files: HashMap::new(),
                ..Default::default()
            },
        )
        .expect("seed store index");
    drop(index);
    OfflineStoreView::new(Arc::new(Mutex::new(
        StoreIndex::open_readonly(store_dir.path()).expect("open store index read-only"),
    )))
}

#[tokio::test]
async fn offline_pick_prefers_the_version_whose_tarball_the_store_holds() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    let body: serde_json::Value = serde_json::from_str(PACKAGE_BODY).expect("parse packument body");
    let integrity =
        body["versions"]["1.0.0"]["dist"]["integrity"].as_str().expect("1.0.0 integrity");

    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(
        cache_dir.path(),
        ABBREVIATED_META_DIR,
        "https://registry.invalid/",
        &preloaded,
    )
    .expect("warm mirror");

    let store_dir = TempDir::new().expect("tempdir");
    let store_view = seed_store(&store_dir, "acme@1.0.0", integrity);

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = offline_ctx(
        &cache_dir,
        Some(&store_view),
        &meta_cache,
        &fetch_locker,
        &http_client,
        &auth_headers,
    );

    let result = pick_package(
        &ctx,
        &range_spec("acme", "^1.0.0"),
        &default_opts("https://registry.invalid/"),
    )
    .await
    .expect("offline pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.0.0");
}

#[tokio::test]
async fn offline_pick_falls_back_to_the_newest_version_when_the_store_holds_none() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");

    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(
        cache_dir.path(),
        ABBREVIATED_META_DIR,
        "https://registry.invalid/",
        &preloaded,
    )
    .expect("warm mirror");

    let store_dir = TempDir::new().expect("tempdir");
    let store_view: OfflineStoreView = OfflineStoreView::new(Arc::new(Mutex::new(
        StoreIndex::open(store_dir.path()).expect("open empty store index"),
    )));

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = offline_ctx(
        &cache_dir,
        Some(&store_view),
        &meta_cache,
        &fetch_locker,
        &http_client,
        &auth_headers,
    );

    let result = pick_package(
        &ctx,
        &range_spec("acme", "^1.0.0"),
        &default_opts("https://registry.invalid/"),
    )
    .await
    .expect("offline pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
}

#[tokio::test]
async fn offline_pick_without_a_store_index_keeps_the_newest_version() {
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");

    let cache_dir = TempDir::new().expect("tempdir");
    persist_meta_to_mirror(
        cache_dir.path(),
        ABBREVIATED_META_DIR,
        "https://registry.invalid/",
        &preloaded,
    )
    .expect("warm mirror");

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx =
        offline_ctx(&cache_dir, None, &meta_cache, &fetch_locker, &http_client, &auth_headers);

    let result = pick_package(
        &ctx,
        &range_spec("acme", "^1.0.0"),
        &default_opts("https://registry.invalid/"),
    )
    .await
    .expect("offline pick succeeds");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
}
