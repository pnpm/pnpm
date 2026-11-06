//! Registry metadata kept in the `SQLite` database a pnpr over the storage
//! reads by default.
//!
//! The fixture storage is seeded in one commit, and a test reads and edits a
//! tree of its own through the same store a running registry reads. Every operation runs on
//! a thread and runtime of its own, so a caller inside an async runtime can
//! use them too.

use node_semver::Version;
use pnpr_config::{HostedStoreConfig, MetadataBackend, default_cache_dir, default_metadata_path};
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    metadata::{
        ArtifactBlob, BatchDocument, BlobFence, PublishNotes, ReleaseRecords, SqlMetadata,
        TagChange, TagUpdate,
    },
};
use serde_json::{Map, Value, json};
use std::{future::Future, path::Path, sync::Arc};

/// How many times an edit retries a write that lost to a concurrent one.
const EDIT_RETRIES: usize = 8;

/// Point `tag` at `version` in a built storage tree — the fixture-side
/// equivalent of the JS harness's `addDistTag`, and the only way to give a
/// package a `latest` other than its highest published version.
///
/// `time.modified` moves with the tag, the same as pnpr's own
/// `PUT /-/package/:pkg/dist-tags/:tag` — it is what the served
/// `Last-Modified` is derived from.
///
/// Only call this on a tree the test owns, never on the process-global
/// storage every other test reads — see [`crate::build_storage_at`].
pub fn set_dist_tag(storage: &Path, package: &str, version: &str, tag: &str) {
    let packument = read_packument(storage, package);
    assert!(
        packument
            .get("versions")
            .and_then(Value::as_object)
            .is_some_and(|versions| versions.contains_key(version)),
        "{package} has no fixture version {version} to tag as {tag}",
    );
    move_tag(storage, package, version, tag);
}

/// Deprecate `package@version` with `message` in a built storage tree.
///
/// Only call this on a tree the test owns, as with [`set_dist_tag`].
pub fn deprecate_version(storage: &Path, package: &str, version: &str, message: &str) {
    edit_packument(storage, package, |packument_object| {
        packument_object
            .get_mut("versions")
            .and_then(|versions| versions.get_mut(version))
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{package} has no fixture version {version}"))
            .insert("deprecated".to_string(), json!(message));
    });
}

/// Drop `version` from the registry metadata of `package` in a built storage
/// tree, so the registry serves the package as one that never published it. A
/// `dist-tags` entry naming it moves to the highest version left. The tarball
/// stays, which no resolution reaches.
///
/// Only call this on a tree the test owns, as with [`set_dist_tag`].
pub fn remove_version(storage: &Path, package: &str, version: &str) {
    edit_packument(storage, package, |packument_object| {
        let versions = packument_object
            .get_mut("versions")
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{package} has fixture versions"));
        assert!(versions.remove(version).is_some(), "{package} has no fixture version {version}");
        let highest = versions
            .keys()
            .filter_map(|left| left.parse::<Version>().ok())
            .max()
            .unwrap_or_else(|| panic!("{package} keeps a version after removing {version}"))
            .to_string();
        move_dist_tags(packument_object, version, &highest);
        if let Some(time) = packument_object.get_mut("time").and_then(Value::as_object_mut) {
            time.remove(version);
        }
    });
}

/// Point every `dist-tags` entry naming `from` at `to`.
fn move_dist_tags(packument_object: &mut Map<String, Value>, from: &str, to: &str) {
    let Some(tags) = packument_object.get_mut("dist-tags").and_then(Value::as_object_mut) else {
        return;
    };
    for tag in tags.values_mut() {
        if tag.as_str() == Some(from) {
            *tag = json!(to);
        }
    }
}

/// Commit `packuments`, by package name, to the metadata of the storage at
/// `storage` in one transaction, whatever it held for them before. A
/// packument the store already holds as it is commits nothing.
pub fn write_packuments(storage: &Path, packuments: Vec<(String, Value)>) {
    write_packuments_into(&default_metadata_path(storage), packuments);
}

/// [`write_packuments`] into the metadata database at `database`.
pub fn write_packuments_into(database: &Path, packuments: Vec<(String, Value)>) {
    commit_packuments(database, packuments, Vec::new());
}

/// [`write_packuments`], mapping each of `archives` to the incarnation of
/// its bytes the storage holds, in the same commit.
pub(crate) fn write_packuments_with_archives(
    storage: &Path,
    packuments: Vec<(String, Value)>,
    archives: Vec<ArtifactBlob>,
) {
    commit_packuments(&default_metadata_path(storage), packuments, archives);
}

fn commit_packuments(
    database: &Path,
    packuments: Vec<(String, Value)>,
    archives: Vec<ArtifactBlob>,
) {
    let path = database.to_path_buf();
    let documents = packuments
        .into_iter()
        .map(|(name, packument)| BatchDocument {
            ecosystem: Ecosystem::Npm,
            name: canonical(&name, Ecosystem::Npm).as_str().to_string(),
            bytes: serde_json::to_vec(&packument).expect("serialize fixture packument"),
            expected: Some(HostedDocumentVersion::Unversioned),
            principal: None,
            records: ReleaseRecords::default(),
        })
        .collect();
    run(async move {
        let database = Arc::new(
            SqlMetadata::open_sqlite(&path)
                .unwrap_or_else(|error| {
                    panic!("open registry metadata at {}: {error}", path.display())
                }),
        );
        let fence = BlobFence { artifacts: archives, references: Vec::new() };
        database
            .commit_fenced_publication(
                "",
                documents,
                Vec::new(),
                PublishNotes::default(),
                None,
                fence,
            )
            .await
            .unwrap_or_else(|error| panic!("write registry packuments: {error}"));
    });
}

/// Where a built storage tree holds the tarball of `package@version`: the
/// incarnation of its bytes the package's tarball is mapped to.
#[must_use]
pub fn tarball_path(storage: &Path, package: &str, version: &str) -> std::path::PathBuf {
    let store = open(storage);
    let name = canonical(package, Ecosystem::Npm);
    let file = name.tarball_name_for_version(version);
    let blob = run(async move {
        store
            .hosted_blob_of(&name, &file)
            .await
            .unwrap_or_else(|error| panic!("read the tarball mapping of {package}: {error}"))
    })
    .unwrap_or_else(|| panic!("no fixture tarball of {package}@{version}"));
    pnpr_storage::layout::local_archive_path(storage, "", Ecosystem::Npm, &blob.key())
}

/// The bytes the storage at `storage` serves for the file `file` of `name` in
/// the `ecosystem` namespace of `org`, through the blob the file is mapped
/// to, or `None` when the package maps no such file.
#[must_use]
pub fn read_hosted_file(
    storage: &Path,
    (org, ecosystem): (&str, Ecosystem),
    name: &str,
    file: &str,
) -> Option<Vec<u8>> {
    let store = open(storage).for_hosted(org, ecosystem);
    let name = canonical(name, ecosystem);
    run(async move {
        let (body, _) = store
            .open_hosted_blob(&name, file)
            .await
            .unwrap_or_else(|error| panic!("open {file} of {}: {error}", name.as_str()))?;
        let bytes = axum::body::to_bytes(body, usize::MAX).await
            .unwrap_or_else(|error| panic!("read {file} of {}: {error}", name.as_str()));
        Some(bytes.to_vec())
    })
}

/// Store `bytes` as the file `file` of `version` of `name` in the
/// `ecosystem` namespace of `org` of the storage at `storage`, and map the
/// file to them, as a publish would. `version` is empty for a file of no
/// version.
pub fn write_hosted_file(
    storage: &Path,
    (org, ecosystem): (&str, Ecosystem),
    (name, version): (&str, &str),
    file: &str,
    bytes: &[u8],
) {
    let store = open(storage).for_hosted(org, ecosystem);
    let name = canonical(name, ecosystem);
    let fail = |error: &dyn std::fmt::Display| -> ! {
        panic!("store {file} of {}: {error}", name.as_str())
    };
    run(async {
        let slot = store
            .reserve_hosted_blob(&name, file)
            .await
            .unwrap_or_else(|error| fail(&error))
            .for_version(version);
        tokio::fs::write(&slot.tmp_path, bytes).await.unwrap_or_else(|error| fail(&error));
        let (id, size) = store.identify_blob_slot(&slot).await.unwrap_or_else(|error| fail(&error));
        let rows = store
            .blob_rows(vec![id.clone()])
            .await
            .unwrap_or_else(|error| fail(&error));
        let row = rows.get(&id).copied();
        let promoted = store
            .promote_blob_slot(slot, (id, size), row.as_ref())
            .await
            .unwrap_or_else(|error| fail(&error));
        store.map_hosted_artifact(promoted.artifact).await.unwrap_or_else(|error| fail(&error));
    });
}

/// The stored packument of `package`, as a running registry would serve it
/// before rewriting its URLs.
#[must_use]
pub fn read_packument(storage: &Path, package: &str) -> Value {
    let bytes = read_hosted_document(storage, "", Ecosystem::Npm, package)
        .unwrap_or_else(|| panic!("no fixture package {package}"));
    serde_json::from_slice(&bytes).expect("parse fixture packument")
}

/// The stored document of `name` in the `ecosystem` namespace of `org`
/// (`""` for the default one) of the storage at `storage`, or `None` when it
/// holds none.
#[must_use]
pub fn read_hosted_document(
    storage: &Path,
    org: &str,
    ecosystem: Ecosystem,
    name: &str,
) -> Option<Vec<u8>> {
    let store = open(storage).for_hosted(org, ecosystem);
    let name = canonical(name, ecosystem);
    run(async move {
        store
            .read_hosted_document(&name)
            .await
            .unwrap_or_else(|error| {
                panic!("read the hosted document of {}: {error}", name.as_str())
            })
    })
}

/// Commit `bytes` as the document of `name` in the `ecosystem` namespace of
/// `org` of the storage at `storage`, whatever it held before.
pub fn write_hosted_document(
    storage: &Path,
    org: &str,
    ecosystem: Ecosystem,
    name: &str,
    bytes: &[u8],
) {
    let store = open(storage).for_hosted(org, ecosystem);
    let name = canonical(name, ecosystem);
    run(async move {
        let version = HostedDocumentVersion::Unversioned;
        store
            .write_hosted_document_if_current(&name, bytes, Some(&version), None)
            .await
            .unwrap_or_else(|error| {
                panic!("write the hosted document of {}: {error}", name.as_str())
            });
    });
}

/// Rewrite the packument of `package` with `edit`, through the store's
/// own rules: a version's accepted manifest never changes, a dropped version
/// is removed, and tags and deprecations change in place.
fn edit_packument(
    storage: &Path,
    package: &str,
    mut edit: impl FnMut(&mut Map<String, Value>) + Send,
) {
    let store = open(storage);
    let name = canonical(package, Ecosystem::Npm);
    run(async move {
        store
            .update_hosted_document_with_retry(&name, EDIT_RETRIES, None, |existing| {
                let bytes = existing.unwrap_or_else(|| panic!("no fixture package {package}"));
                let mut packument: Value =
                    serde_json::from_slice(bytes).expect("parse fixture packument");
                edit(packument.as_object_mut().expect("fixture packument is an object"));
                Ok(Some(serde_json::to_vec(&packument)?))
            })
            .await
            .unwrap_or_else(|error| panic!("edit fixture packument of {package}: {error}"));
    });
}

/// Point `tag` of `package` at `version`.
fn move_tag(storage: &Path, package: &str, version: &str, tag: &str) {
    let store = open(storage);
    let name = canonical(package, Ecosystem::Npm);
    run(async move {
        let change = TagChange { tag, target: Some(version), principal: None };
        let update = store
            .change_dist_tag(&name, change)
            .await
            .unwrap_or_else(|error| panic!("tag {package}@{version} as {tag}: {error}"));
        assert!(!matches!(update, TagUpdate::NotFound), "no fixture package {package}");
    });
}

fn open(storage: &Path) -> Storage {
    let metadata = MetadataBackend::Sqlite(default_metadata_path(storage));
    Storage::new(
        &HostedStoreConfig::Fs,
        storage.to_path_buf(),
        default_cache_dir(storage),
        &metadata,
    )
    .unwrap_or_else(|error| panic!("open fixture storage at {}: {error}", storage.display()))
}

fn canonical(package: &str, ecosystem: Ecosystem) -> CanonicalPackageName {
    CanonicalPackageName::parse(package, ecosystem)
        .unwrap_or_else(|error| panic!("package name {package:?}: {error}"))
}

/// Run `work` to completion on a thread and runtime of its own.
fn run<Output: Send>(work: impl Future<Output = Output> + Send) -> Output {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("create fixture metadata runtime")
                    .block_on(work)
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}
