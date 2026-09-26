use std::path::Path;

use pnpm_network::MetadataCacheScope;

use super::{
    ABBREVIATED_META_DIR, AuthHeaders, InMemoryPackageMetaCache, PACKAGE_BODY, PackageMetaCache,
    PickPackageContext, PickPackageError, RetryOpts, TempDir, ThrottledClient, assert_eq,
    default_opts, get_pkg_mirror_path, load_meta, metadata_cache_key, persist_meta_to_mirror,
    pick_package, range_spec, shared_packument_fetch_locker,
};

/// Damages the fragment of `acme@1.1.0` in the mirror `persist_meta_to_mirror`
/// wrote, leaving the file length and the index spans intact so only the
/// fragment's own bytes fail to parse. Returns the damaged file's bytes.
fn damage_mirror_fragment(cache_dir: &Path, registry: &str) -> Vec<u8> {
    let path = get_pkg_mirror_path(cache_dir, ABBREVIATED_META_DIR, registry, "acme")
        .expect("mirror path");
    let mut bytes = std::fs::read(&path).expect("read mirror");
    // An unescaped quote inside 1.1.0's integrity string: still the same
    // number of bytes, no longer parsable JSON.
    let marker = b"sha512-BBBB";
    let at = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("1.1.0 fragment in the mirror");
    bytes[at + 3] = b'"';
    std::fs::write(&path, &bytes).expect("write damaged mirror");
    bytes
}

/// Offline, a damaged fragment leaves the mirror as unusable as a missing
/// one — and there is no network to replace it from.
#[tokio::test]
async fn offline_over_a_damaged_mirror_fragment_errors() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .expect(0)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");
    damage_mirror_fragment(cache_dir.path(), &registry);

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

    let err = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("offline + damaged mirror = error");
    assert!(matches!(err, PickPackageError::NoOfflineMeta { .. }), "got {err:?}");
    mock.assert_async().await;
}

/// The etag lives in the mirror's intact headers record, so a conditional
/// request keeps answering `304` over a damaged body. Online, the resolver
/// has to ask again with the cache bypassed and rewrite the file.
#[tokio::test]
async fn a_damaged_fragment_behind_a_304_is_replaced_by_a_bypassing_refetch() {
    let mut server = mockito::Server::new_async().await;
    let conditional = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"cached""#)
        .match_header("cache-control", mockito::Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let bypassing = server
        .mock("GET", "/acme")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mut preloaded: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    preloaded.etag = Some(r#"W/"cached""#.to_string());
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &preloaded)
        .expect("warm mirror");
    let damaged = damage_mirror_fragment(cache_dir.path(), &registry);

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
        .expect("ok");
    assert_eq!(result.picked_package.expect("picked").version.to_string(), "1.1.0");
    conditional.assert_async().await;
    bypassing.assert_async().await;

    let path = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("mirror path");
    let healed = std::fs::read(&path).expect("read mirror");
    assert_ne!(healed, damaged, "the bypassing refetch rewrites the damaged mirror");
    let reloaded = load_meta(&path).expect("reload the healed mirror");
    assert!(reloaded.versions.get("1.1.0").is_some());
    assert!(!reloaded.versions.has_corrupt_mirror_fragment());
}

const UNPARSABLE_VERSION_PACKAGE_BODY: &str = r#"{
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
            "version": "1.1.0"
        }
    }
}"#;

#[tokio::test]
async fn registry_shaped_undecodable_version_errors_instead_of_serving_a_corrupt_pick() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(UNPARSABLE_VERSION_PACKAGE_BODY)
        .expect(2)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
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

    let err = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &default_opts(&registry))
        .await
        .expect_err("a twice-undecodable version must error, not silently pick");
    assert!(matches!(err, PickPackageError::CorruptMetadataMirror { .. }), "got {err:?}");
    let cache_key =
        metadata_cache_key(&MetadataCacheScope::Public, &registry, "acme", false, false);
    assert!(
        meta_cache.get(&cache_key).is_none(),
        "a document known to be corrupt must never reach the in-memory cache",
    );
    mock.assert_async().await;
}
