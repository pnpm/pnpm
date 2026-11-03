//! Registry metadata kept in the `SQLite` database a pnpr over the storage
//! reads by default.
//!
//! The fixture storage is seeded in one commit, and a test reads and edits a
//! tree of its own through the same store a running registry reads. Every operation runs on
//! a thread and runtime of its own, so a caller inside an async runtime can
//! use them too.

use pnpr_config::{HostedStoreConfig, MetadataBackend, default_cache_dir, default_metadata_path};
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    HostedDocumentVersion, Storage,
    metadata::{BatchDocument, PublishNotes, SqlMetadata, TagChange, TagUpdate},
};
use serde_json::{Map, Value};
use std::{future::Future, path::Path, sync::Arc};

/// How many times an edit retries a write that lost to a concurrent one.
const EDIT_RETRIES: usize = 8;

/// Commit `packuments`, by package name, to the metadata of the storage at
/// `storage` in one transaction, whatever it held for them before. A
/// packument the store already holds as it is commits nothing.
pub fn write_packuments(storage: &Path, packuments: Vec<(String, Value)>) {
    let path = default_metadata_path(storage);
    let documents = packuments
        .into_iter()
        .map(|(name, packument)| BatchDocument {
            ecosystem: Ecosystem::Npm,
            name: canonical(&name, Ecosystem::Npm).as_str().to_string(),
            bytes: serde_json::to_vec(&packument).expect("serialize fixture packument"),
            expected: Some(HostedDocumentVersion::Unversioned),
            principal: None,
            records: Default::default(),
        })
        .collect();
    run(async move {
        let database = Arc::new(
            SqlMetadata::open_sqlite(&path)
                .unwrap_or_else(|error| {
                    panic!("open registry metadata at {}: {error}", path.display())
                }),
        );
        database
            .commit_publication("", documents, Vec::new(), PublishNotes::default(), None)
            .await
            .unwrap_or_else(|error| panic!("write registry packuments: {error}"));
    });
}

/// The stored packument of `package`, as a running registry would serve it
/// before rewriting its URLs.
pub fn read_packument(storage: &Path, package: &str) -> Value {
    let bytes = read_hosted_document(storage, "", Ecosystem::Npm, package)
        .unwrap_or_else(|| panic!("no fixture package {package}"));
    serde_json::from_slice(&bytes).expect("parse fixture packument")
}

/// The stored document of `name` in the `ecosystem` namespace of `org`
/// (`""` for the default one) of the storage at `storage`, or `None` when it
/// holds none.
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
pub(crate) fn edit_packument(
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
pub(crate) fn move_tag(storage: &Path, package: &str, version: &str, tag: &str) {
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
    Storage::new(&HostedStoreConfig::Fs, storage.to_path_buf(), default_cache_dir(storage))
        .and_then(|store| {
            store.with_metadata(&MetadataBackend::Sqlite(default_metadata_path(storage)))
        })
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
