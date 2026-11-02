//! Conformance tests of the `SQLite` metadata backend.

mod concurrency;

use crate::{
    DocumentWrite, HostedDocumentVersion, Storage,
    metadata::{Generation, RenderStats, TagChange, TagUpdate},
};
use pnpr_config::{HostedStoreConfig, MetadataBackend, StorageConfig};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
use tempfile::TempDir;

pub(super) fn config(tmp: &TempDir) -> StorageConfig {
    StorageConfig {
        hosted_dir: tmp.path().join("storage"),
        cache_dir: tmp.path().join("cache"),
        cache: pnpr_config::CacheConfig::default(),
        hosted_backend: HostedStoreConfig::Fs,
        metadata: MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    }
}

pub(super) fn storage(tmp: &TempDir) -> Storage {
    Storage::from_config(&config(tmp)).unwrap()
}

pub(super) fn npm(name: &str) -> CanonicalPackageName {
    CanonicalPackageName::parse(name, Ecosystem::Npm).unwrap()
}

/// A packument of `name` with one manifest per version, `latest` on the last.
pub(super) fn packument(name: &str, versions: &[&str]) -> Value {
    let manifests: serde_json::Map<String, Value> = versions
        .iter()
        .map(|version| {
            let manifest = json!({
                "name": name,
                "version": version,
                "readme": format!("# {name} {version}"),
                "dist": { "integrity": format!("sha512-{version}"), "tarball": "t.tgz" },
            });
            ((*version).to_string(), manifest)
        })
        .collect();
    let mut time =
        json!({ "created": "2026-01-01T00:00:00.000Z", "modified": "2026-01-02T00:00:00.000Z" });
    for version in versions {
        time[*version] = json!("2026-01-01T00:00:00.000Z");
    }
    json!({
        "name": name,
        "description": "a package",
        "dist-tags": { "latest": versions.last().copied().unwrap_or_default() },
        "versions": manifests,
        "time": time,
        "maintainers": [{ "name": "someone" }],
    })
}

pub(super) async fn write(storage: &Storage, name: &str, document: &Value) -> DocumentWrite {
    let current = storage
        .read_hosted_document_for_update(&npm(name))
        .await
        .unwrap();
    let bytes = serde_json::to_vec(document).unwrap();
    storage
        .write_hosted_document_if_current(
            &npm(name),
            &bytes,
            current.map(|doc| doc.version).as_ref(),
        )
        .await
        .unwrap()
}

pub(super) async fn read(storage: &Storage, name: &str) -> Value {
    let bytes = storage
        .read_hosted_document(&npm(name))
        .await
        .unwrap()
        .expect("document stored");
    serde_json::from_slice(&bytes).unwrap()
}

async fn generation(storage: &Storage, name: &str) -> Option<Generation> {
    storage
        .read_hosted_package_view(&npm(name))
        .await
        .unwrap()
        .and_then(|view| view.generation)
}

fn stats(storage: &Storage) -> RenderStats {
    storage
        .sqlite_metadata()
        .unwrap()
        .render_stats()
}

#[tokio::test]
async fn a_packument_reads_back_as_written() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let mut document = packument("pkg", &["1.0.0", "1.1.0"]);
    document["versions"]["1.0.0"]["deprecated"] = json!("use 1.1.0");
    document["time"]["0.9.0"] = json!("2025-01-01T00:00:00.000Z");
    assert_eq!(write(&storage, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(read(&storage, "pkg").await, document);
    assert_eq!(storage.hosted_package_names().await.unwrap(), vec!["pkg".to_string()]);
}

#[tokio::test]
async fn an_accepted_version_cannot_change() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let document = packument("pkg", &["1.0.0"]);
    write(&storage, "pkg", &document).await;
    let mut changed = document.clone();
    changed["versions"]["1.0.0"]["dependencies"] = json!({ "evil": "1.0.0" });
    let current = storage
        .read_hosted_document_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    let err = storage
        .write_hosted_document_if_current(
            &npm("pkg"),
            &serde_json::to_vec(&changed).unwrap(),
            Some(&current.version),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::VersionAlreadyPublished { .. }), "{err:?}");
    assert_eq!(read(&storage, "pkg").await, document);
}

#[tokio::test]
async fn an_identical_write_commits_no_generation() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let document = packument("pkg", &["1.0.0"]);
    write(&storage, "pkg", &document).await;
    let first = generation(&storage, "pkg").await;
    assert_eq!(write(&storage, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(generation(&storage, "pkg").await, first);
}

#[tokio::test]
async fn generations_advance_per_catalog() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let org_a = storage.for_hosted("org-a", Ecosystem::Npm);
    let org_b = storage.for_hosted("org-b", Ecosystem::Npm);
    write(&org_a, "one", &packument("one", &["1.0.0"])).await;
    write(&org_a, "two", &packument("two", &["1.0.0"])).await;
    write(&org_b, "one", &packument("one", &["1.0.0"])).await;
    let token = |catalog: &str, value| Some(Generation { catalog: catalog.to_string(), value });
    assert_eq!(generation(&org_a, "one").await, token("org-a", 1));
    assert_eq!(generation(&org_a, "two").await, token("org-a", 2));
    assert_eq!(generation(&org_b, "one").await, token("org-b", 1));
    assert_eq!(token("org-b", 1).unwrap().to_string(), "org-b:1");
    assert!(
        org_a
            .read_hosted_document(&npm("other"))
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn a_write_from_a_stale_read_conflicts() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    let stale = storage
        .read_hosted_document_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    let bytes = serde_json::to_vec(&packument("pkg", &["1.0.0", "3.0.0"])).unwrap();
    let outcome = storage
        .write_hosted_document_if_current(&npm("pkg"), &bytes, Some(&stale.version))
        .await
        .unwrap();
    assert_eq!(outcome, DocumentWrite::Conflict);
    let absent = storage
        .write_hosted_document_if_current(&npm("pkg"), &bytes, None)
        .await
        .unwrap();
    assert_eq!(absent, DocumentWrite::Conflict);
    let any = Some(&HostedDocumentVersion::Unversioned);
    let unconditional = storage.write_hosted_document_if_current(&npm("pkg"), &bytes, any).await;
    assert_eq!(unconditional.unwrap(), DocumentWrite::Written);
}

#[tokio::test]
async fn a_tag_change_reuses_every_fragment() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "1.1.0", "2.0.0"])).await;
    read(&storage, "pkg").await;
    assert_eq!(stats(&storage).fragments_rendered, 3);
    let change = TagChange { tag: "next", target: Some("2.0.0"), principal: Some("alice") };
    let update = storage
        .change_dist_tag(&npm("pkg"), change)
        .await
        .unwrap();
    let generation_after = generation(&storage, "pkg").await;
    assert_eq!(update, TagUpdate::Committed { generation: generation_after, moved: true });
    let document = read(&storage, "pkg").await;
    assert_eq!(document["dist-tags"], json!({ "latest": "2.0.0", "next": "2.0.0" }));
    let stats = stats(&storage);
    assert_eq!(stats.fragments_rendered, 3, "{stats:?}");
    assert_eq!(stats.fragments_reused, 3, "{stats:?}");
    assert_eq!(stats.views_assembled, 2, "{stats:?}");
}

#[tokio::test]
async fn a_deprecation_rerenders_one_fragment() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let mut document = packument("pkg", &["1.0.0", "2.0.0"]);
    write(&storage, "pkg", &document).await;
    read(&storage, "pkg").await;
    document = read(&storage, "pkg").await;
    assert_eq!(stats(&storage).views_reused, 1);
    document["versions"]["1.0.0"]["deprecated"] = json!("old");
    write(&storage, "pkg", &document).await;
    assert_eq!(read(&storage, "pkg").await["versions"]["1.0.0"]["deprecated"], "old");
    assert_eq!(stats(&storage).fragments_rendered, 3);
    document["versions"]["1.0.0"]
        .as_object_mut()
        .unwrap()
        .remove("deprecated");
    write(&storage, "pkg", &document).await;
    assert!(read(&storage, "pkg").await["versions"]["1.0.0"].get("deprecated").is_none());
}

#[tokio::test]
async fn tag_moves_are_recorded() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    let set = TagChange { tag: "latest", target: Some("1.0.0"), principal: Some("alice") };
    storage
        .change_dist_tag(&npm("pkg"), set)
        .await
        .unwrap();
    let unchanged = storage
        .change_dist_tag(&npm("pkg"), set)
        .await
        .unwrap();
    let remove = TagChange { tag: "latest", target: None, principal: Some("bob") };
    storage
        .change_dist_tag(&npm("pkg"), remove)
        .await
        .unwrap();
    let history = storage
        .sqlite_metadata()
        .unwrap()
        .tag_history(&npm("pkg"), "", "latest")
        .unwrap();
    let moves: Vec<_> = history
        .iter()
        .map(|entry| {
            (entry.previous.as_deref(), entry.target.as_deref(), entry.principal.as_deref())
        })
        .collect();
    assert_eq!(
        moves,
        [
            (None, Some("2.0.0"), None),
            (Some("2.0.0"), Some("1.0.0"), Some("alice")),
            (Some("1.0.0"), None, Some("bob")),
        ],
    );
    let generation = Some(Generation { catalog: String::new(), value: 2 });
    assert_eq!(unchanged, TagUpdate::Committed { generation, moved: false });
    let missing = storage
        .change_dist_tag(&npm("absent"), set)
        .await
        .unwrap();
    assert_eq!(missing, TagUpdate::NotFound);
}

#[tokio::test]
async fn removing_versions_and_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    write(&storage, "pkg", &packument("pkg", &["2.0.0"])).await;
    let document = read(&storage, "pkg").await;
    assert!(document["versions"].get("1.0.0").is_none());
    assert!(document["time"].get("1.0.0").is_none());
    assert!(
        storage
            .remove_package(&npm("pkg"))
            .await
            .unwrap(),
    );
    assert!(
        storage
            .read_hosted_document(&npm("pkg"))
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        storage
            .hosted_package_names()
            .await
            .unwrap()
            .is_empty(),
    );
    let mut republished = packument("pkg", &["1.0.0"]);
    republished["versions"]["1.0.0"]["description"] = json!("a different manifest");
    write(&storage, "pkg", &republished).await;
    assert_eq!(read(&storage, "pkg").await, republished);
}

#[tokio::test]
async fn another_ecosystem_stores_its_document_whole() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp).for_hosted("org", Ecosystem::Cargo);
    let name = CanonicalPackageName::parse("serde", Ecosystem::Cargo).unwrap();
    storage.write_hosted_document_if_current(&name, b"not json", None).await.unwrap();
    assert_eq!(
        storage
            .read_hosted_document(&name)
            .await
            .unwrap()
            .unwrap(),
        b"not json",
    );
    let tag = TagChange { tag: "latest", target: Some("1.0.0"), principal: None };
    assert_eq!(storage.change_dist_tag(&name, tag).await.unwrap(), TagUpdate::NotFound);
}
