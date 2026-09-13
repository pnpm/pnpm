//! Crash-atomic commit journal for the publish flow.
//!
//! A publish stages every blob into a tmp file and computes the document of
//! each package it touches in memory; making the result visible then takes
//! several non-atomic steps — one rename/upload per blob, one document write
//! per package. A crash in the middle of those steps could leave a blob that
//! no document mentions, or some packages of a batch published and others
//! not. The journal closes that window: before anything is promoted, the full
//! intent — the computed document bytes, revision references, and locations
//! of the staged tmp files — is persisted under `.pnpr-journal/<txn>/` and
//! sealed with a single atomic rename of the `commit` marker.
//! [`PublishJournal::commit`] then applies it, and [`recover_publish_journal`]
//! runs at startup, before the server accepts requests: sealed transactions
//! are applied (every step is idempotent) and unsealed ones are rolled back,
//! so a publish is either fully visible or fully absent.
//!
//! Every surface publishes this way — an npm packument, a Cargo crate
//! document, a Python project document. The journal carries each package's
//! document as opaque bytes and asks the caller's [`HostedDocuments`] to
//! merge them into what the store holds, so the merge rule stays with the
//! ecosystem whose format it belongs to.
//!
//! Once a transaction is sealed, the publish *will* become visible —
//! if applying it fails at request time (e.g. the S3 backend is briefly
//! unreachable), the client sees an error but the sealed transaction
//! completes on the next startup. An operator can abort a sealed-but-
//! unapplied transaction by deleting its directory.
//!
//! Applying merges the journaled document into whatever is on disk (rather
//! than overwriting it), so replaying an old sealed transaction cannot erase
//! what was published between the failed apply and the restart.

mod apply;
mod transaction_files;

use transaction_files::{
    cleanup_lost_tmp_paths, revision_ref_owner, roll_back, sync_dir, txn_id, write_transaction,
};

use crate::{
    BlobFinalize, BlobSlot, COMMIT_DOCUMENT_WRITE_RETRIES, DocumentUpdate, DocumentWrite,
    HostedDocumentVersion, HostedRevisionRefWrite, Storage, is_canonical_revision_ref_owner,
    unique_tmp_path,
};
use pnpr_config::Config;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{fs, io::AsyncWriteExt};

/// Name of the journal directory. It sits inside the local root that
/// also holds the staged tmp files (the hosted store root on the fs
/// backend, the cache scratch on the S3 backend); the leading dot
/// keeps it out of the package-listing walk, and no valid package name
/// can collide with it.
pub(crate) const JOURNAL_DIR: &str = ".pnpr-journal";

const COMMIT_MARKER: &str = "commit";
const MANIFEST_FILE: &str = "manifest.json";

/// Per-process counter feeding [`txn_id`] so two transactions sealed in
/// the same millisecond get distinct directories.
static TXN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    packages: Vec<ManifestPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestPackage {
    name: String,
    /// The ecosystem whose document format this package's document is in.
    /// An entry that names none holds an npm packument.
    #[serde(default)]
    ecosystem: Ecosystem,
    /// Hosted-org storage namespace this package publishes into, or `None` for
    /// the flat (path-less) hosted store. Recovery namespaces the roll-forward
    /// by it so a crash mid-commit promotes into the right org. Defaulted for
    /// back-compat with journals written before org registries existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    org: Option<String>,
    /// File inside the transaction directory holding the computed
    /// document bytes.
    #[serde(alias = "packument_file")]
    document_file: String,
    #[serde(alias = "tarballs")]
    blobs: Vec<ManifestBlob>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revision_refs: Vec<JournaledRevisionRef>,
}

impl ManifestPackage {
    fn id(&self) -> PackageId {
        PackageId {
            ecosystem: self.ecosystem,
            name: self.name.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestBlob {
    /// Canonical on-disk filename (`<basename>-<version>.tgz` for npm).
    filename: String,
    /// The staged tmp file holding the verified bytes.
    tmp_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournaledRevisionRef {
    pub filename: String,
    pub digest: String,
    pub ref_id: String,
    pub bytes: Vec<u8>,
}

/// One package of a publish about to be committed, borrowed from the
/// handler's staged state.
pub struct JournaledPublish<'publish> {
    pub name: &'publish CanonicalPackageName,
    /// Hosted-org storage namespace, or `None` for the flat hosted store.
    pub org: Option<&'publish str>,
    pub ecosystem: Ecosystem,
    /// The document the publish computed, to be merged into the stored one.
    pub document: &'publish [u8],
    /// The version of the stored document `document` was computed from, when
    /// the publish read one. While the store is still at that version the
    /// commit writes `document` as it is; otherwise it merges.
    pub base_version: Option<&'publish HostedDocumentVersion>,
    pub slots: &'publish [BlobSlot],
    pub revision_refs: &'publish [JournaledRevisionRef],
}

/// One hosted document to bring up to date as a transaction is applied.
pub struct DocumentMerge<'txn> {
    pub ecosystem: Ecosystem,
    pub name: &'txn CanonicalPackageName,
    /// The document as the store holds it, `None` when the package has none.
    pub existing: Option<&'txn [u8]>,
    /// The document the transaction computed when it was sealed.
    pub journaled: &'txn [u8],
    /// Canonical filenames of the blobs the transaction failed to place.
    /// An entry backed by one of them must not be recorded: the bytes it
    /// describes are not the ones the store serves.
    pub lost_blobs: &'txn HashSet<String>,
}

/// How each ecosystem's hosted document is merged. The journal carries
/// documents as opaque bytes, so the surface that owns the format supplies
/// this. Startup recovery re-runs the same merge, which is why
/// [`recover_publish_journal`] takes it too.
pub trait HostedDocuments: Send + Sync {
    /// The bytes to store for the merged document, or `None` when the merge
    /// records nothing — a transaction that lost every blob it staged leaves
    /// the stored document exactly as it is, and writes none where there was
    /// none.
    fn merge(&self, merge: DocumentMerge<'_>) -> Result<Option<Vec<u8>>>;
}

/// A package a transaction could not record, named the way the journal
/// addressed it: the same name in two ecosystems is two packages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub ecosystem: Ecosystem,
    pub name: String,
}

/// A blob this transaction could not place, because another writer already
/// owned its immutable slot with different bytes, and the package whose entry
/// would have described it.
#[derive(Debug)]
pub struct LostBlob {
    pub package: PackageId,
    pub filename: String,
}

/// What a committed transaction could not record. Its document was written
/// without those entries, so the store never advertises what it does not
/// hold; the surface decides what to report to the publisher.
#[derive(Debug, Default)]
pub struct CommitOutcome {
    /// The blobs whose immutable slot another writer already owned.
    pub lost_blobs: Vec<LostBlob>,
    /// Set when an entry could not claim a digest-reference slot, to the
    /// limit that was reached.
    pub reference_limit: Option<usize>,
    /// Packages whose merge left the stored document exactly as it was:
    /// every entry this transaction journaled for them was already recorded,
    /// or was lost with its blob. Nothing of theirs became newly visible.
    pub unrecorded: Vec<PackageId>,
}

/// What one attempt at applying a transaction got done. A failed attempt
/// reports its progress too: the retry needs to know which documents this
/// transaction has already written to tell its own entries from another
/// writer's.
#[derive(Debug, Default)]
struct ApplyProgress {
    outcome: CommitOutcome,
    wrote_documents: HashSet<PackageId>,
}

/// Handle to the journal directory of one [`Storage`].
pub struct PublishJournal {
    root: PathBuf,
}

/// A sealed transaction: the journal entry is durable and carries the
/// commit marker, so the publish it holds will become visible — through
/// [`Self::apply`] here, or through startup recovery.
struct SealedTxn {
    dir: PathBuf,
    revision_ref_owner: String,
    /// The stored-document version each sealed package was computed from,
    /// positionally. Empty when startup recovery reopens the transaction,
    /// which always merges instead.
    base_versions: Vec<Option<HostedDocumentVersion>>,
}

impl PublishJournal {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
        }
    }

    /// Seal `packages` and make them visible: one journaled transaction over
    /// every blob and document the publish touches. Until the seal nothing
    /// has been promoted, so a failure there takes the staged tmp files and
    /// the half-written entry with it and leaves no trace; past it the
    /// transaction is committed, so a failure to apply leaves the entry for
    /// startup recovery rather than undoing it.
    pub async fn commit(
        &self,
        storage: &Storage,
        packages: &[JournaledPublish<'_>],
        documents: &dyn HostedDocuments,
    ) -> Result<CommitOutcome> {
        let txn = match self.seal(packages).await {
            Ok(txn) => txn,
            Err(err) => {
                for slot in packages
                    .iter()
                    .flat_map(|package| package.slots)
                {
                    let _ = fs::remove_file(&slot.tmp_path).await;
                }
                return Err(err);
            }
        };
        let dir = txn.dir.clone();
        let mut first = ApplyProgress::default();
        let Err(err) = txn.apply(storage, documents, &mut first).await else {
            return Ok(first.outcome);
        };
        tracing::warn!(%err, "publish apply failed after the seal; retrying it");
        // An apply can stop with some of the batch already promoted, and past
        // the seal there is nothing to undo. Run the same idempotent apply
        // once more so a running server does not leave the batch half-visible
        // until the next restart; a second failure keeps the sealed entry for
        // startup recovery and reports the failure that started it.
        let Ok(txn) = SealedTxn::reopen(dir) else {
            return Err(err);
        };
        let mut retry = ApplyProgress::default();
        if txn.apply(storage, documents, &mut retry).await.is_err() {
            return Err(err);
        }
        // An entry the first attempt wrote is this transaction's own, and the
        // retry finding it in place says nothing about another writer.
        // Reporting it would tell a publisher their publish duplicated itself.
        retry.outcome.unrecorded.retain(|package| !first.wrote_documents.contains(package));
        Ok(retry.outcome)
    }

    /// Persist the full intent of the publish and seal it with the
    /// commit marker. After this returns `Ok`, the publish is
    /// committed: either the caller applies it now, or startup
    /// recovery does.
    async fn seal(&self, packages: &[JournaledPublish<'_>]) -> Result<SealedTxn> {
        let revision_ref_owner = txn_id();
        let dir = self.root.join(&revision_ref_owner);
        if let Err(err) = write_transaction(&dir, packages).await {
            // Nothing of an unsealed transaction may become visible. Startup
            // recovery would roll this one back, but removing it here keeps a
            // publisher that keeps failing from piling up directories.
            let _ = fs::remove_dir_all(&dir).await;
            return Err(err);
        }
        let base_versions = packages
            .iter()
            .map(|package| package.base_version.cloned())
            .collect();
        Ok(SealedTxn {
            dir,
            revision_ref_owner,
            base_versions,
        })
    }
}

impl PublishJournal {
    /// Bring every journal entry to a consistent state: sealed transactions
    /// are applied, unsealed ones rolled back. Must run before the server
    /// accepts requests — it takes no package locks.
    pub async fn recover(&self, storage: &Storage, documents: &dyn HostedDocuments) -> Result<()> {
        let mut entries = match fs::read_dir(&self.root).await {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let mut txn_dirs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                txn_dirs.push(entry.path());
            }
        }
        // Transaction ids start with a zero-padded millisecond
        // timestamp, so the lexical order is the seal order.
        txn_dirs.sort();
        for dir in txn_dirs {
            // Never treat "can't tell" as unsealed: an I/O error probing
            // the marker must not send a possibly-sealed transaction to
            // rollback, which would delete an already-committed publish.
            // Abort recovery so startup fails loudly instead.
            if fs::try_exists(dir.join(COMMIT_MARKER)).await? {
                SealedTxn::reopen(dir.clone())?
                    .apply(storage, documents, &mut ApplyProgress::default())
                    .await?;
                tracing::info!(txn = %dir.display(), "applied publish journal entry");
            } else {
                roll_back(&dir).await;
                tracing::info!(txn = %dir.display(), "rolled publish journal entry back");
            }
        }
        Ok(())
    }
}

/// The shared inputs of one journal apply.
struct ApplyContext<'a> {
    documents: &'a dyn HostedDocuments,
    progress: &'a mut ApplyProgress,
}

/// The store and canonical name one journaled package's writes land under.
struct PackageTarget {
    store: Storage,
    name: CanonicalPackageName,
}

/// Promote the package's staged blobs, returning the filenames another writer
/// had already claimed with different bytes.
async fn promote_blobs<'a>(
    target: &PackageTarget,
    package: &'a ManifestPackage,
    lost_tmp_paths: &mut Vec<&'a std::path::Path>,
) -> Result<HashSet<String>> {
    let mut lost_blobs = HashSet::new();
    for blob in &package.blobs {
        // A missing tmp file was already promoted before the crash, so skip
        // it. But never read an I/O error as "missing": that would skip
        // promotion, write the document anyway, and delete the journal entry —
        // advertising a blob with nothing on disk and no journal state left to
        // retry from. Propagate it instead so the apply aborts and the entry
        // survives for a later attempt.
        if !fs::try_exists(&blob.tmp_path).await? {
            continue;
        }
        let slot = BlobSlot::from_parts(
            blob.tmp_path.clone(),
            target.name.clone(),
            blob.filename.clone(),
        );
        match target.store.finalize_blob_slot(slot).await? {
            BlobFinalize::Written | BlobFinalize::AlreadyIdentical => {}
            // Another writer placed different bytes under this filename. Keep
            // the tmp file so a retry detects the same conflict, and leave the
            // entry out of the merge.
            BlobFinalize::Conflict => {
                lost_tmp_paths.push(blob.tmp_path.as_path());
                lost_blobs.insert(blob.filename.clone());
            }
        }
    }
    Ok(lost_blobs)
}

/// Bring the publish journal of the storage configured in `config` to a
/// consistent state. `pnpr::serve` and `pnpr::serve_listener`
/// call this before binding; embedders that build a router directly
/// should call it themselves on startup.
pub async fn recover_publish_journal(
    config: &Config,
    documents: &dyn HostedDocuments,
) -> Result<()> {
    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )?;
    storage.publish_journal().recover(&storage, documents).await
}

#[cfg(test)]
mod tests;
