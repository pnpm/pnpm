//! Conformance tests of the SQL metadata backend. [`conformance!`] runs each
//! test against every adapter: `SQLite`, a libSQL primary, and a libSQL
//! primary read through an embedded replica of each instance that syncs only
//! when it must.

#[macro_use]
mod conformance;
mod blobs;
mod concurrency;
#[cfg(feature = "backend-libsql")]
mod libsql;
mod log;
mod notes;
mod publications;
mod reclaims;
mod schema;

use crate::{
    DocumentWrite, HostedDocumentVersion, Storage,
    metadata::{Generation, RenderStats, TagChange, TagUpdate},
};
use pnpr_config::{HostedStoreConfig, MetadataBackend, StorageConfig};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use serde_json::{Value, json};
#[cfg(feature = "backend-libsql")]
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

/// A metadata database under test. Every [`storage`] of it is another pnpr
/// instance on the same database.
pub(super) struct Db {
    tmp: TempDir,
    adapter: Adapter,
    /// Instances opened so far, each with a replica of its own.
    #[cfg(feature = "backend-libsql")]
    instances: AtomicUsize,
}

enum Adapter {
    Sqlite,
    #[cfg(feature = "backend-libsql")]
    Libsql {
        server: libsql::Server,
        /// Whether each instance reads through an embedded replica.
        replica: bool,
    },
}

impl Db {
    pub(super) fn sqlite() -> Self {
        Self::new(TempDir::new().unwrap(), Adapter::Sqlite)
    }

    #[cfg(feature = "backend-libsql")]
    pub(super) async fn libsql(replica: bool) -> Self {
        let server = libsql::server().await;
        Self::new(TempDir::new().unwrap(), Adapter::Libsql { server, replica })
    }

    /// A connection to the database that skips the schema check, as an
    /// operator's own client would be.
    pub(super) fn adapter(&self) -> Box<dyn super::adapter::Adapter> {
        match &self.adapter {
            Adapter::Sqlite => {
                Box::new(super::sqlite::SqliteAdapter::open(&sqlite_path(&self.tmp)).unwrap())
            }
            #[cfg(feature = "backend-libsql")]
            Adapter::Libsql { server, .. } => {
                Box::new(super::libsql::LibsqlAdapter::new(server.settings(None, 3600)))
            }
        }
    }

    /// The connection of another pnpr instance on the database. On libSQL
    /// with replicas it reads through a replica of its own, as an instance
    /// of [`storage`] does.
    pub(super) fn instance_adapter(&self) -> Box<dyn super::adapter::Adapter> {
        match &self.adapter {
            #[cfg(feature = "backend-libsql")]
            Adapter::Libsql { server, replica: true } => {
                let instance = self.instances.fetch_add(1, Ordering::SeqCst);
                let replica = format!("instance-{instance}");
                Box::new(super::libsql::LibsqlAdapter::new(server.settings(Some(&replica), 3600)))
            }
            _ => self.adapter(),
        }
    }

    fn new(tmp: TempDir, adapter: Adapter) -> Self {
        Self {
            tmp,
            adapter,
            #[cfg(feature = "backend-libsql")]
            instances: AtomicUsize::new(0),
        }
    }
}

pub(super) fn config(tmp: &TempDir) -> StorageConfig {
    StorageConfig {
        hosted_dir: tmp.path().join("storage"),
        cache_dir: tmp.path().join("cache"),
        cache: pnpr_config::CacheConfig::default(),
        hosted_backend: HostedStoreConfig::Fs,
        metadata: MetadataBackend::Sqlite(sqlite_path(tmp)),
    }
}

fn sqlite_path(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join("metadata.sqlite")
}

/// Another pnpr instance on `db`, with a database handle of its own. A
/// libSQL instance with a replica syncs it at startup, and afterwards only
/// when a read must see more than it holds.
pub(super) fn storage(db: &Db) -> Storage {
    match &db.adapter {
        Adapter::Sqlite => Storage::from_config(&config(&db.tmp)).unwrap(),
        #[cfg(feature = "backend-libsql")]
        Adapter::Libsql { server, replica } => {
            let instance = db.instances.fetch_add(1, Ordering::SeqCst);
            let replica = replica.then(|| format!("instance-{instance}"));
            let config = config(&db.tmp);
            Storage::from_parts(
                std::sync::Arc::new(crate::Store::new(config.hosted_dir)),
                crate::Store::new(config.cache_dir),
                server.instance(replica.as_deref(), 3600),
            )
        }
    }
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
    try_write(storage, name, document).await.unwrap()
}

pub(super) async fn try_write(
    storage: &Storage,
    name: &str,
    document: &Value,
) -> pnpr_error::Result<DocumentWrite> {
    let current = storage
        .read_hosted_document_for_update(&npm(name))
        .await
        .unwrap();
    let bytes = serde_json::to_vec(document).unwrap();
    storage.write_hosted_document_if_current(
        &npm(name),
        &bytes,
        current.map(|doc| doc.version).as_ref(),
        None,
    )
    .await
}

pub(super) async fn read(storage: &Storage, name: &str) -> Value {
    let bytes = storage
        .read_hosted_document(&npm(name))
        .await
        .unwrap()
        .expect("document stored");
    serde_json::from_slice(&bytes).unwrap()
}

pub(super) async fn generation(storage: &Storage, name: &str) -> Option<Generation> {
    storage
        .read_hosted_package_view(&npm(name))
        .await
        .unwrap()
        .map(|view| view.generation)
}

fn stats(storage: &Storage) -> RenderStats {
    storage.sql_metadata().render_stats()
}

async fn a_packument_reads_back_as_written(db: Db) {
    let storage = storage(&db);
    let mut document = packument("pkg", &["1.0.0", "1.1.0"]);
    document["versions"]["1.0.0"]["deprecated"] = json!("use 1.1.0");
    document["time"]["0.9.0"] = json!("2025-01-01T00:00:00.000Z");
    assert_eq!(write(&storage, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(read(&storage, "pkg").await, document);
    assert_eq!(storage.hosted_package_names().await.unwrap(), vec!["pkg".to_string()]);
}

async fn an_accepted_version_cannot_change(db: Db) {
    let storage = storage(&db);
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
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::VersionAlreadyPublished { .. }), "{err:?}");
    assert_eq!(read(&storage, "pkg").await, document);
}

async fn an_archive_manifest_record_is_the_accepted_interpretation(db: Db) {
    let storage = storage(&db);
    let mut document = packument("pkg", &["1.0.0", "1.1.0"]);
    let record = json!({
        "profile": "npm-install-manifest/1",
        "packageJson": "sha512-manifest",
        "npmShrinkwrap": "sha512-shrinkwrap",
    });
    document["versions"]["1.1.0"]["_archiveManifest"] = record.clone();
    write(&storage, "pkg", &document).await;

    let rows = accepted_manifests(&storage).await;
    let [(_, unverified_profile, unverified_inputs, _), (_, profile, inputs, manifest)] =
        rows.as_slice()
    else {
        panic!("two accepted manifests: {rows:?}");
    };
    assert_eq!((unverified_profile.as_str(), unverified_inputs.as_str()), ("submitted", "{}"));
    assert_eq!(profile, "npm-install-manifest/1");
    assert_eq!(
        serde_json::from_str::<Value>(inputs).unwrap(),
        json!({ "packageJson": "sha512-manifest", "npmShrinkwrap": "sha512-shrinkwrap" }),
    );
    assert!(!manifest.contains("_archiveManifest"), "{manifest}");
    assert_eq!(read(&storage, "pkg").await, document);

    let mut changed = document.clone();
    changed["versions"]["1.1.0"]["_archiveManifest"]["packageJson"] = json!("sha512-other");
    let err = try_write(&storage, "pkg", &changed).await.unwrap_err();
    assert!(matches!(err, RegistryError::VersionAlreadyPublished { .. }), "{err:?}");
    let mut dropped = document.clone();
    dropped["versions"]["1.1.0"]
        .as_object_mut()
        .unwrap()
        .shift_remove("_archiveManifest");
    let err = try_write(&storage, "pkg", &dropped).await.unwrap_err();
    assert!(matches!(err, RegistryError::VersionAlreadyPublished { .. }), "{err:?}");
}

async fn an_archive_manifest_record_must_name_a_profile(db: Db) {
    let storage = storage(&db);
    for record in [
        json!("npm-install-manifest/1"),
        json!({ "packageJson": "sha512-a" }),
        json!({
            "profile": "submitted"
        }),
    ] {
        let mut document = packument("pkg", &["1.0.0"]);
        document["versions"]["1.0.0"]["_archiveManifest"] = record;
        let err = try_write(&storage, "pkg", &document).await.unwrap_err();
        assert!(matches!(err, RegistryError::BadRequest { .. }), "{err:?}");
    }
}

/// The version, profile, inputs, and manifest of each accepted manifest, as
/// the primary holds them.
async fn accepted_manifests(storage: &Storage) -> Vec<(String, String, String, String)> {
    let metadata = std::sync::Arc::clone(storage.sql_metadata());
    tokio::task::spawn_blocking(move || {
        let mut rows = Vec::new();
        metadata.adapter
            .write(&mut |connection| {
                rows = connection
                    .query(
                        "SELECT version, profile, inputs, manifest FROM accepted_manifests
                         ORDER BY version",
                        &[],
                    )?
                    .into_iter()
                    .map(|mut row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                    .collect::<pnpr_error::Result<_>>()?;
                Ok(())
            })
            .unwrap();
        rows
    })
    .await
    .unwrap()
}

async fn an_identical_write_commits_no_generation(db: Db) {
    let storage = storage(&db);
    let document = packument("pkg", &["1.0.0"]);
    write(&storage, "pkg", &document).await;
    let first = generation(&storage, "pkg").await;
    assert_eq!(write(&storage, "pkg", &document).await, DocumentWrite::Written);
    assert_eq!(generation(&storage, "pkg").await, first);
}

async fn generations_advance_per_catalog(db: Db) {
    let storage = storage(&db);
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

async fn a_write_from_a_stale_read_conflicts(db: Db) {
    let storage = storage(&db);
    write(&storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    let stale = storage
        .read_hosted_document_for_update(&npm("pkg"))
        .await
        .unwrap()
        .unwrap();
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    let bytes = serde_json::to_vec(&packument("pkg", &["1.0.0", "3.0.0"])).unwrap();
    let outcome = storage
        .write_hosted_document_if_current(&npm("pkg"), &bytes, Some(&stale.version), None)
        .await
        .unwrap();
    assert_eq!(outcome, DocumentWrite::Conflict);
    let absent = storage
        .write_hosted_document_if_current(&npm("pkg"), &bytes, None, None)
        .await
        .unwrap();
    assert_eq!(absent, DocumentWrite::Conflict);
    let any = Some(&HostedDocumentVersion::Unversioned);
    let unconditional =
        storage.write_hosted_document_if_current(&npm("pkg"), &bytes, any, None).await;
    assert_eq!(unconditional.unwrap(), DocumentWrite::Written);
}

async fn a_tag_change_reuses_every_fragment(db: Db) {
    let storage = storage(&db);
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

async fn a_deprecation_rerenders_one_fragment(db: Db) {
    let storage = storage(&db);
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

async fn a_version_reads_without_the_package_view(db: Db) {
    let storage = storage(&db);
    let mut document = packument("pkg", &["1.0.0", "2.0.0"]);
    document["versions"]["1.0.0"]["_archiveManifest"] =
        json!({ "profile": "npm-install-manifest/1", "packageJson": "sha512-input" });
    write(&storage, "pkg", &document).await;
    let read_version = async |version| {
        let manifest = storage
            .read_hosted_version(&npm("pkg"), version)
            .await
            .unwrap()?;
        Some(serde_json::from_str::<Value>(&manifest).unwrap())
    };
    let mut expected = document["versions"]["1.0.0"].clone();
    expected
        .as_object_mut()
        .unwrap()
        .remove("readme");
    assert_eq!(read_version("1.0.0").await, Some(expected.clone()));
    assert_eq!(read_version("3.0.0").await, None);
    assert_eq!(read_version("latest").await, None);
    let rendered = stats(&storage);
    assert_eq!((rendered.fragments_rendered, rendered.views_assembled), (1, 0), "{rendered:?}");
    // A deprecation is not part of the version's accepted manifest, so it
    // neither shows nor renders the fragment again.
    document = read(&storage, "pkg").await;
    document["versions"]["1.0.0"]["deprecated"] = json!("old");
    write(&storage, "pkg", &document).await;
    assert_eq!(read_version("1.0.0").await, Some(expected));
    let reused = stats(&storage);
    assert_eq!(reused.fragments_reused, 1, "{reused:?}");
}

async fn tag_moves_are_recorded(db: Db) {
    let storage = storage(&db);
    let published = serde_json::to_vec(&packument("pkg", &["1.0.0", "2.0.0"])).unwrap();
    storage
        .write_hosted_document_if_current(&npm("pkg"), &published, None, Some("carol"))
        .await
        .unwrap();
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
        .sql_metadata()
        .tag_history(&npm("pkg"), "", "latest")
        .await
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
            (None, Some("2.0.0"), Some("carol")),
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

async fn removing_versions_and_packages(db: Db) {
    let storage = storage(&db);
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
    // A reclaimed name is published with new versions.
    let republished = packument("pkg", &["3.0.0"]);
    write(&storage, "pkg", &republished).await;
    assert_eq!(read(&storage, "pkg").await, republished);
}

async fn a_removed_version_is_never_accepted_again(db: Db) {
    let storage = storage(&db);
    write(&storage, "pkg", &packument("pkg", &["1.0.0", "2.0.0"])).await;
    // Unpublishing one version, then the whole package.
    write(&storage, "pkg", &packument("pkg", &["2.0.0"])).await;
    let mut changed = packument("pkg", &["1.0.0", "2.0.0"]);
    changed["versions"]["1.0.0"]["dist"]["integrity"] = json!("sha512-other");
    let refused = try_write(&storage, "pkg", &changed).await;
    assert!(
        matches!(refused, Err(RegistryError::CannotRepublishUnpublishedVersion { ref version, .. }) if version == "1.0.0"),
        "{refused:?}",
    );
    assert!(
        storage
            .remove_package(&npm("pkg"))
            .await
            .unwrap(),
    );
    for version in ["1.0.0", "2.0.0"] {
        let refused = try_write(&storage, "pkg", &packument("pkg", &[version])).await;
        assert!(
            matches!(refused, Err(RegistryError::CannotRepublishUnpublishedVersion { .. })),
            "{version}: {refused:?}",
        );
    }
    // The refused writes committed nothing, and a restarted store still
    // refuses.
    assert!(
        storage
            .read_hosted_document(&npm("pkg"))
            .await
            .unwrap()
            .is_none(),
    );
    drop(storage);
    let restarted = self::storage(&db);
    let refused = try_write(&restarted, "pkg", &packument("pkg", &["2.0.0"])).await;
    assert!(matches!(refused, Err(RegistryError::CannotRepublishUnpublishedVersion { .. })));
}

async fn another_ecosystem_stores_its_document_whole(db: Db) {
    let storage = storage(&db).for_hosted("org", Ecosystem::Cargo);
    let name = CanonicalPackageName::parse("serde", Ecosystem::Cargo).unwrap();
    storage.write_hosted_document_if_current(&name, b"not json", None, None).await.unwrap();
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

conformance! {
    a_packument_reads_back_as_written,
    an_accepted_version_cannot_change,
    an_identical_write_commits_no_generation,
    generations_advance_per_catalog,
    a_write_from_a_stale_read_conflicts,
    a_tag_change_reuses_every_fragment,
    a_deprecation_rerenders_one_fragment,
    tag_moves_are_recorded,
    a_version_reads_without_the_package_view,
    removing_versions_and_packages,
    a_removed_version_is_never_accepted_again,
    an_archive_manifest_record_is_the_accepted_interpretation,
    an_archive_manifest_record_must_name_a_profile,
    another_ecosystem_stores_its_document_whole,
}
