use std::{collections::BTreeMap, path::Path, sync::Arc};

use pnpm_config::NetworkPolicy;
use pnpm_lockfile::{RegistryOptions, RegistryServerType};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_registry::{InstallV2Entry, PackageVersion};
use serde_json::{Value, json};

use super::{record_revision, with_release_record};
use crate::{
    InMemoryPackageMetaCache, PackumentFetchLocker, RegistryRevisionSelector,
    pick_package::PickPackageContext, shared_packument_fetch_locker,
};

const INTEGRITY: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";

/// The state one install's picks share, with `registry` declared
/// `serverType: pnpr` unless `declared` is false.
struct Install {
    http_client: ThrottledClient,
    auth_headers: AuthHeaders,
    meta_cache: InMemoryPackageMetaCache,
    fetch_locker: PackumentFetchLocker,
    registry_options: BTreeMap<String, RegistryOptions>,
}

impl Install {
    fn new(registry: &str, declared: bool) -> Self {
        let server_type = declared.then_some(RegistryServerType::Pnpr);
        let options = RegistryOptions { server_type, supports_time_field: None };
        Install {
            http_client: ThrottledClient::default(),
            auth_headers: AuthHeaders::default(),
            meta_cache: InMemoryPackageMetaCache::default(),
            fetch_locker: shared_packument_fetch_locker(),
            registry_options: BTreeMap::from([(registry.to_string(), options)]),
        }
    }

    fn ctx<'a>(
        &'a self,
        cache_dir: Option<&'a Path>,
        network: NetworkPolicy,
    ) -> PickPackageContext<'a, InMemoryPackageMetaCache> {
        PickPackageContext {
            pnpr_format: Some(crate::PnprFormatPolicy {
                registry_options: &self.registry_options,
                minimum_release_age: None,
            }),
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            cache_policy: crate::MetadataCachePolicy { network, ..Default::default() },
            store_view: None,
            metadata: crate::MetadataRequestContext {
                meta_cache: &self.meta_cache,
                fetch_locker: &self.fetch_locker,
                cache_dir,
                http: crate::MetadataHttpClient {
                    http_client: &self.http_client,
                    auth_headers: &self.auth_headers,
                    retry_opts: RetryOpts::default(),
                },
            },
        }
    }
}

/// A version as pnpr's document lists it: `entry` names how, and `dist`
/// adds to its `dist`.
fn listed(name: &str, entry: InstallV2Entry, dist: &Value) -> Arc<PackageVersion> {
    let mut dist = dist.clone();
    dist["pnpr"] = json!({ "installV2": entry });
    let version = json!({ "name": name, "version": "1.0.0", "deprecated": "use 2", "dist": dist });
    Arc::new(serde_json::from_value(version).expect("version"))
}

fn record(name: &str, version: &str) -> String {
    json!({
        "name": name,
        "version": version,
        "integrity": INTEGRITY,
        "manifest": { "dependencies": { "dep": "^2.0.0" } },
    })
    .to_string()
}

fn integrity_of(version: &PackageVersion) -> Option<String> {
    version.dist.integrity.as_ref().map(ToString::to_string)
}

#[tokio::test]
async fn an_external_version_is_read_once_per_install_from_its_release_record() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_body(record("acme", "1.0.0"))
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let install = Install::new(&registry, true);
    let ctx = install.ctx(None, NetworkPolicy::default());
    for _ in 0..2 {
        let picked = listed("acme", InstallV2Entry::External, &json!({}));
        let picked = with_release_record(&ctx, &registry, picked, None).await.expect("read");
        assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
        // The deprecation may change after publication, so it comes from the
        // document, never from the release record.
        assert_eq!(picked.deprecated.as_deref(), Some("use 2"));
        assert!(
            picked.dependencies
                .as_ref()
                .is_some_and(|deps| deps.contains_key("dep"))
        );
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn a_scoped_name_is_escaped_in_the_record_address() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/@acme%2Fwidget/1.0.0+r0")
        .with_body(record("@acme/widget", "1.0.0"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let install = Install::new(&registry, true);
    let ctx = install.ctx(None, NetworkPolicy::default());
    let picked = listed("@acme/widget", InstallV2Entry::External, &json!({}));
    with_release_record(&ctx, &registry, picked, None).await.expect("read");
    mock.assert_async().await;
}

#[tokio::test]
async fn a_cached_record_is_read_offline() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_body(record("acme", "1.0.0"))
        .expect(1)
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let first = Install::new(&registry, true);
    let picked = listed("acme", InstallV2Entry::External, &json!({}));
    let ctx = first.ctx(Some(cache_dir.path()), NetworkPolicy::default());
    with_release_record(&ctx, &registry, picked, None).await.expect("fetched");

    let second = Install::new(&registry, true);
    let picked = listed("acme", InstallV2Entry::External, &json!({}));
    let ctx = second.ctx(Some(cache_dir.path()), NetworkPolicy::Offline);
    let picked = with_release_record(&ctx, &registry, picked, None).await.expect("cached");
    assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
    mock.assert_async().await;
}

#[tokio::test]
async fn a_complete_version_and_an_undeclared_registry_read_no_record() {
    // No server: neither pick may reach the network.
    let registry = "http://127.0.0.1:9/";
    let install = Install::new(registry, true);
    let ctx = install.ctx(None, NetworkPolicy::default());
    let picked = listed("acme", InstallV2Entry::Complete, &json!({ "integrity": INTEGRITY }));
    let picked = with_release_record(&ctx, registry, picked, None).await.expect("kept");
    assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));

    let undeclared = Install::new(registry, false);
    let ctx = undeclared.ctx(None, NetworkPolicy::default());
    let picked = listed("acme", InstallV2Entry::External, &json!({}));
    with_release_record(&ctx, registry, picked, None).await.expect("kept");
}

#[tokio::test]
async fn a_record_of_another_version_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_body(record("acme", "1.0.1"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let install = Install::new(&registry, true);
    let ctx = install.ctx(None, NetworkPolicy::default());
    let picked = listed("acme", InstallV2Entry::External, &json!({}));
    let error = with_release_record(&ctx, &registry, picked, None).await.expect_err("rejected");
    assert!(error.to_string().contains("acme@1.0.1"), "{error}");
}

#[tokio::test]
async fn an_override_of_another_revision_reads_that_revisions_record() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme/1.0.0+r0")
        .with_body(record("acme", "1.0.0"))
        .create_async()
        .await;
    let registry = format!("{}/", server.url());
    let install = Install::new(&registry, true);
    let ctx = install.ctx(None, NetworkPolicy::default());
    let picked = listed("acme", InstallV2Entry::Complete, &json!({ "revision": 2 }));
    let requested = RegistryRevisionSelector::Valid(0);
    let picked = with_release_record(&ctx, &registry, picked, Some(&requested))
        .await
        .expect("read");
    assert_eq!(picked.dist.revision, Some(json!(0)));
    assert_eq!(integrity_of(&picked).as_deref(), Some(INTEGRITY));
    mock.assert_async().await;
}

#[test]
fn the_record_a_pick_needs() {
    let external = listed("acme", InstallV2Entry::External, &json!({ "revision": 2 }));
    assert_eq!(record_revision(&external, None), Some(2));
    let complete = listed("acme", InstallV2Entry::Complete, &json!({ "revision": 2 }));
    assert_eq!(record_revision(&complete, None), None);
    let selected = RegistryRevisionSelector::Valid(2);
    assert_eq!(record_revision(&complete, Some(&selected)), None);
    let original = RegistryRevisionSelector::Valid(0);
    assert_eq!(record_revision(&complete, Some(&original)), Some(0));
    // A version outside the revision protocol has only its original.
    let ordinary = listed("acme", InstallV2Entry::Complete, &json!({}));
    assert_eq!(record_revision(&ordinary, Some(&RegistryRevisionSelector::Valid(1))), None);
    let ordinary_external = listed("acme", InstallV2Entry::External, &json!({}));
    assert_eq!(record_revision(&ordinary_external, None), Some(0));
}
