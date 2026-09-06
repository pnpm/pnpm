//! The hosted document each surface keeps per package, and the journaled
//! publish that records a blob in one.
//!
//! A hosted registry stores one document per package — an npm packument, a
//! crate's index entries, a Python project's file list — beside the blobs it
//! serves. Publishing a blob and recording it in the document has to be one
//! operation: a blob no document mentions is invisible, and a document entry
//! with no blob behind it is a broken download. [`store_hosted_artifact`]
//! makes it one, through the same commit journal the npm publish flow uses,
//! and [`RegistryDocuments`] gives that journal the per-ecosystem merge rule
//! it re-runs when a transaction is applied after a crash.

use super::{AppState, hosted_read_namespace};
use pnpr_cargo::{CrateDocument, crate_filename};
use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_pypi::ProjectDocument;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    journal::{DocumentMerge, HostedDocuments, JournaledPublish},
    publish::merge_journaled_packument,
};
use std::{collections::HashSet, slice};

/// The per-project document a hosted registry keeps in its document slot: a
/// crate's index entries, a Python project's file list. Read through
/// [`read_hosted_document`] and written through [`store_hosted_artifact`].
pub(super) trait HostedDocument: Sized {
    /// The ecosystem whose surface publishes this document.
    const ECOSYSTEM: Ecosystem;

    fn empty(name: &str) -> Self;
    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error>;
    fn to_bytes(&self) -> Vec<u8>;

    /// Take on every entry of `addition` this document does not already have,
    /// and report whether that added anything. An entry is skipped when its
    /// blob filename is in `lost_blobs`, which names the blobs a transaction
    /// failed to place: those bytes are not the ones the store serves, so
    /// nothing may point at them.
    ///
    /// Entries already here win, which is what makes a re-applied publish
    /// safe: what was published in the meantime stays, and only what is
    /// genuinely missing is added.
    fn merge(&mut self, addition: Self, lost_blobs: &HashSet<String>) -> bool;
}

impl HostedDocument for CrateDocument {
    const ECOSYSTEM: Ecosystem = Ecosystem::Cargo;

    fn empty(name: &str) -> Self {
        CrateDocument::new(name)
    }

    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        CrateDocument::parse(bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        CrateDocument::to_bytes(self)
    }

    fn merge(&mut self, addition: Self, lost_blobs: &HashSet<String>) -> bool {
        let before = self.versions.len();
        for entry in addition.versions {
            if self.version(&entry.vers).is_some()
                || lost_blobs.contains(&crate_filename(&entry.name, &entry.vers))
            {
                continue;
            }
            // The document carries the name as first published, not the
            // lowercase key it is stored under.
            if self.versions.is_empty() {
                self.name.clone_from(&entry.name);
            }
            self.versions.push(entry);
        }
        self.versions.len() != before
    }
}

impl HostedDocument for ProjectDocument {
    const ECOSYSTEM: Ecosystem = Ecosystem::Pypi;

    fn empty(name: &str) -> Self {
        ProjectDocument::new(name)
    }

    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        ProjectDocument::parse(bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        ProjectDocument::to_bytes(self)
    }

    fn merge(&mut self, addition: Self, lost_blobs: &HashSet<String>) -> bool {
        let before = self.files.len();
        for file in addition.files {
            if self.file(&file.filename).is_some() || lost_blobs.contains(&file.filename) {
                continue;
            }
            self.files.push(file);
        }
        self.files.len() != before
    }
}

/// Every hosted document format pnpr serves, as the publish journal sees
/// them. The journal carries documents as opaque bytes; this resolves the
/// ecosystem it recorded back to the merge rule for that format, both when a
/// publish commits and when startup recovery re-applies a sealed one.
pub(super) struct RegistryDocuments;

impl HostedDocuments for RegistryDocuments {
    fn merge(&self, merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>, RegistryError> {
        match merge.ecosystem {
            Ecosystem::Npm => merge_journaled_packument(&merge),
            Ecosystem::Cargo => merge_into_stored::<CrateDocument>(&merge),
            Ecosystem::Pypi => merge_into_stored::<ProjectDocument>(&merge),
        }
    }
}

fn merge_into_stored<Document: HostedDocument>(
    merge: &DocumentMerge<'_>,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let journaled = Document::parse(merge.journaled).map_err(RegistryError::Json)?;
    let mut stored = match merge.existing {
        Some(bytes) => Document::parse(bytes).map_err(RegistryError::Json)?,
        None => Document::empty(merge.name.as_str()),
    };
    Ok(stored.merge(journaled, merge.lost_blobs).then(|| stored.to_bytes()))
}

/// The hosted document of `key` through `source`'s read gate: `None` when the
/// project is absent (or masked from `identity`).
pub(super) async fn read_hosted_document<Document: HostedDocument>(
    state: &AppState,
    identity: &Identity,
    source: &str,
    key: &PackageName,
) -> Result<Option<Document>, RegistryError> {
    let org = hosted_read_namespace(state, identity, source, key.as_str())?;
    state
        .inner
        .storage
        .for_hosted(&org)
        .read_hosted_packument(key)
        .await?
        .map(|bytes| Document::parse(&bytes).map_err(RegistryError::Json))
        .transpose()
}

/// Publish one blob: store `bytes` as `filename` under `key` in the hosted
/// namespace `org` and record `addition` — the document holding just that
/// blob's entry — in the project's document, as one journaled transaction.
///
/// Writers of the same key on this instance are serialized by the package
/// lock; `refuse` rejects a document the publish must not land on (an entry
/// already present) before anything is written. Across instances the blob's
/// immutable slot decides: a publish whose bytes lose it reports the
/// conflict, and one that finds its entry already recorded by the writer that
/// won answers `refuse` on the document that writer left.
pub(super) async fn store_hosted_artifact<Document: HostedDocument>(
    state: &AppState,
    org: &str,
    key: &PackageName,
    filename: &str,
    bytes: &[u8],
    refuse: impl Fn(&Document) -> Result<(), RegistryError>,
    addition: Document,
) -> Result<(), RegistryError> {
    let _guard = state.inner.package_locks.lock(key.as_str()).await;
    let storage = state.inner.storage.for_hosted(org);
    let stored = storage.read_hosted_packument_for_update(key).await?;
    let mut document = match &stored {
        Some(stored) => Document::parse(&stored.bytes).map_err(RegistryError::Json)?,
        None => Document::empty(key.as_str()),
    };
    refuse(&document)?;
    document.merge(addition, &HashSet::new());
    let document = document.to_bytes();

    let slot = storage.reserve_hosted_tarball(key, filename).await?;
    tokio::fs::write(&slot.tmp_path, bytes).await?;
    let outcome = state
        .inner
        .storage
        .publish_journal()
        .commit(
            &state.inner.storage,
            &[JournaledPublish {
                name: key,
                org: Some(org),
                ecosystem: Document::ECOSYSTEM,
                document: &document,
                base_version: stored.as_ref().map(|stored| &stored.version),
                slots: slice::from_ref(&slot),
                revision_refs: &[],
            }],
            &RegistryDocuments,
        )
        .await?;
    if outcome.lost_blobs.iter().any(|lost| lost == filename) {
        return Err(RegistryError::PackumentWriteConflict { package: key.as_str().to_string() });
    }
    if !outcome.unrecorded.is_empty()
        && let Some(stored) = storage.read_hosted_packument(key).await?
    {
        refuse(&Document::parse(&stored).map_err(RegistryError::Json)?)?;
    }
    Ok(())
}
