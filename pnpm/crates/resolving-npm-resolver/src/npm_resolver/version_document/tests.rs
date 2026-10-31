use std::sync::Arc;

use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_registry::PackageVersion;

use crate::{
    InMemoryPackageMetaCache, npm_resolver::with_version_document,
    pick_package::PickPackageContext, shared_packument_fetch_locker,
};

const INTEGRITY: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";

fn version(dist: &serde_json::Value) -> Arc<PackageVersion> {
    let version = serde_json::json!({ "name": "acme", "version": "1.0.0", "dist": dist });
    Arc::new(serde_json::from_value(version).expect("version"))
}

fn deprecated_version(dist: &serde_json::Value) -> Arc<PackageVersion> {
    let version = serde_json::json!({
        "name": "acme",
        "version": "1.0.0",
        "deprecated": "use 2",
        "dist": dist,
    });
    Arc::new(serde_json::from_value(version).expect("version"))
}

fn integrity_of(version: &PackageVersion) -> Option<String> {
    version.dist.integrity.as_ref().map(ToString::to_string)
}

#[tokio::test]
async fn an_external_version_is_read_once_from_the_version_document() {
    let mut server = mockito::Server::new_async().await;
    let version_doc = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "name": "acme",
                "version": "1.0.0",
                "dependencies": { "dep": "^2.0.0" },
                "dist": { "integrity": INTEGRITY, "tarball": format!("{}/acme/-/acme-1.0.0.tgz", server.url()) },
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy::default(),
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: None,
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };

    for _ in 0..2 {
        let picked = deprecated_version(&serde_json::json!({ "pnpr": { "details": "external" } }));
        let picked = with_version_document(&ctx, &registry, picked).await.expect("hydrated");
        assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
        // Deprecation may change after publication, so it comes from the
        // packument, never from the release record.
        assert_eq!(picked.deprecated.as_deref(), Some("use 2"));
        assert_eq!(
            picked.dependencies
                .as_ref()
                .and_then(|dependencies| dependencies.get("dep"))
                .map(String::as_str),
            Some("^2.0.0"),
        );
    }
    version_doc.assert_async().await;
}

#[tokio::test]
async fn an_inline_dist_is_kept() {
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy::default(),
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: None,
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };
    // No server: a version with its integrity inline, or with none at all
    // (computed while fetching the tarball), needs no version document.
    let picked = version(&serde_json::json!({ "integrity": INTEGRITY }));
    let picked = with_version_document(&ctx, "http://127.0.0.1:9/", picked)
        .await
        .expect("kept");
    assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
    let picked = version(&serde_json::json!({}));
    let picked = with_version_document(&ctx, "http://127.0.0.1:9/", picked)
        .await
        .expect("kept");
    assert_eq!(integrity_of(&picked), None);
}

#[tokio::test]
async fn a_version_document_of_another_version_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let _version_doc = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_status(200)
        .with_body(
            serde_json::json!({ "name": "acme", "version": "1.0.1", "dist": { "integrity": INTEGRITY } })
                .to_string(),
        )
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy::default(),
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: None,
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };
    let picked = version(&serde_json::json!({ "pnpr": { "details": "external" } }));
    let error = with_version_document(&ctx, &registry, picked).await.expect_err("rejected");
    assert!(error.to_string().contains("acme@1.0.1"), "{error}");
}

#[tokio::test]
async fn the_selected_revision_addresses_the_release_record() {
    let mut server = mockito::Server::new_async().await;
    let _version_doc = server
        .mock("GET", "/acme/1.0.0+r2")
        .with_status(200)
        .with_body(
            serde_json::json!({ "name": "acme", "version": "1.0.0", "dist": { "integrity": INTEGRITY } })
                .to_string(),
        )
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = PickPackageContext {
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        cache_policy: crate::MetadataCachePolicy::default(),
        store_view: None,
        metadata: crate::MetadataRequestContext {
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: None,
            http: crate::MetadataHttpClient {
                http_client: &http_client,
                auth_headers: &auth_headers,
                retry_opts: RetryOpts::default(),
            },
        },
    };
    let picked = version(&serde_json::json!({ "revision": 2, "pnpr": { "details": "external" } }));
    let picked = with_version_document(&ctx, &registry, picked).await.expect("hydrated");
    assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
    assert_eq!(picked.dist.revision, Some(serde_json::json!(2)));
}
