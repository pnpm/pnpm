use std::sync::Arc;

use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_registry::{Package, PackageVersion};

use crate::{
    InMemoryPackageMetaCache,
    npm_resolver::{PickedFromRegistry, RegistryPick, with_external_dist},
    pick_package::PickPackageContext,
    shared_packument_fetch_locker,
};

const INTEGRITY: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";

fn picked(dist: &serde_json::Value) -> RegistryPick {
    let version: PackageVersion = serde_json::from_value(
        serde_json::json!({ "name": "acme", "version": "1.0.0", "dist": dist }),
    )
    .expect("version");
    let meta: Package = serde_json::from_value(
        serde_json::json!({ "name": "acme", "dist-tags": {}, "versions": {} }),
    )
    .expect("packument");
    RegistryPick::Picked(PickedFromRegistry {
        meta: Arc::new(meta),
        version: Arc::new(version),
        trust_downgrades_skipped: Vec::new(),
    })
}

fn integrity_of(pick: &RegistryPick) -> Option<String> {
    match pick {
        RegistryPick::Picked(picked) => {
            picked.version.dist.integrity.as_ref().map(ToString::to_string)
        }
        RegistryPick::NoMatchingVersion(_) => None,
    }
}

#[tokio::test]
async fn an_external_dist_is_read_once_from_the_version_document() {
    let mut server = mockito::Server::new_async().await;
    let version_doc = server
        .mock("GET", "/acme/1.0.0")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "name": "acme",
                "version": "1.0.0",
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
        let pick = picked(&serde_json::json!({ "pnpr": { "details": "external" } }));
        let pick = with_external_dist(&ctx, &registry, pick).await.expect("hydrated");
        assert_eq!(integrity_of(&pick).as_deref(), Some(INTEGRITY));
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
    let pick = picked(&serde_json::json!({ "integrity": INTEGRITY }));
    let pick = with_external_dist(&ctx, "http://127.0.0.1:9/", pick).await.expect("kept");
    assert_eq!(integrity_of(&pick).as_deref(), Some(INTEGRITY));
    let pick = picked(&serde_json::json!({}));
    let pick = with_external_dist(&ctx, "http://127.0.0.1:9/", pick).await.expect("kept");
    assert_eq!(integrity_of(&pick), None);
}
