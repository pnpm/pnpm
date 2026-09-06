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

use crate::{
    BlobFinalize, BlobSlot, COMMIT_DOCUMENT_WRITE_RETRIES, DocumentUpdate, DocumentWrite,
    HostedDocumentVersion, HostedRevisionRefWrite, Storage, is_canonical_revision_ref_owner,
    unique_tmp_path,
};
use pnpr_config::Config;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::PackageName;
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
        PackageId { ecosystem: self.ecosystem, name: self.name.clone() }
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
    pub name: &'publish PackageName,
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
    pub name: &'txn PackageName,
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
        Self { root }
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
                for slot in packages.iter().flat_map(|package| package.slots) {
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
        let Ok(txn) = SealedTxn::reopen(dir) else { return Err(err) };
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
        let base_versions = packages.iter().map(|package| package.base_version.cloned()).collect();
        Ok(SealedTxn { dir, revision_ref_owner, base_versions })
    }
}

/// Write the transaction's documents and manifest and seal them with the
/// commit marker, the single atomic rename that commits the publish.
async fn write_transaction(dir: &Path, packages: &[JournaledPublish<'_>]) -> Result<()> {
    fs::create_dir_all(dir).await?;
    let mut manifest = Manifest { packages: Vec::with_capacity(packages.len()) };
    for (index, package) in packages.iter().enumerate() {
        let document_file = format!("document-{index}.json");
        write_synced(&dir.join(&document_file), package.document).await?;
        manifest.packages.push(ManifestPackage {
            name: package.name.as_str().to_string(),
            ecosystem: package.ecosystem,
            org: package.org.map(str::to_string),
            document_file,
            blobs: package
                .slots
                .iter()
                .map(|slot| ManifestBlob {
                    filename: slot.filename().to_string(),
                    tmp_path: slot.tmp_path.clone(),
                })
                .collect(),
            revision_refs: package.revision_refs.to_vec(),
        });
    }
    write_synced(&dir.join(MANIFEST_FILE), &serde_json::to_vec_pretty(&manifest)?).await?;
    let _ = sync_dir(dir).await;
    // The seal itself: a single same-directory rename, atomic on
    // POSIX. Recovery treats a directory without this marker as an
    // aborted transaction and rolls it back.
    let marker = dir.join(COMMIT_MARKER);
    let marker_tmp = unique_tmp_path(&marker);
    write_synced(&marker_tmp, b"").await?;
    fs::rename(&marker_tmp, &marker).await?;
    let _ = sync_dir(dir).await;
    Ok(())
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

impl SealedTxn {
    /// Reopen a sealed transaction from its journal directory, the way
    /// startup recovery does: with no base versions, so every document is
    /// merged into what the store holds rather than written over it.
    fn reopen(dir: PathBuf) -> Result<Self> {
        let revision_ref_owner = revision_ref_owner(&dir)?.to_string();
        Ok(Self { dir, revision_ref_owner, base_versions: Vec::new() })
    }

    /// Run every step of the sealed transaction that has not run yet, then
    /// remove the journal entry. Each step tolerates having already run
    /// before a crash: a tmp file that is gone was already promoted, and the
    /// document is merged into what the store holds rather than overwriting
    /// it, so an interrupted apply just runs again — which is what startup
    /// recovery does.
    async fn apply(
        self,
        storage: &Storage,
        documents: &dyn HostedDocuments,
        progress: &mut ApplyProgress,
    ) -> Result<()> {
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(self.dir.join(MANIFEST_FILE)).await?)?;
        let outcome = &mut progress.outcome;
        let mut lost_tmp_paths = Vec::new();
        for (index, package) in manifest.packages.iter().enumerate() {
            let name = PackageName::parse(&package.name)?;
            // Promote into the package's hosted namespace (or the flat store
            // when it has none), so the commit and a later startup recovery
            // land in exactly the store the publish targeted.
            let store = match &package.org {
                Some(org) => storage.for_hosted(org),
                None => storage.clone(),
            };
            let mut lost_blobs = HashSet::new();
            for blob in &package.blobs {
                // A missing tmp file was already promoted before the crash, so
                // skip it. But never read an I/O error as "missing": that would
                // skip promotion, write the document anyway, and delete the
                // journal entry — advertising a blob with nothing on disk and
                // no journal state left to retry from. Propagate it instead so
                // the apply aborts and the entry survives for a later attempt.
                if fs::try_exists(&blob.tmp_path).await? {
                    let slot = BlobSlot::from_parts(
                        blob.tmp_path.clone(),
                        name.clone(),
                        blob.filename.clone(),
                    );
                    match store.finalize_blob_slot(slot).await? {
                        BlobFinalize::Written | BlobFinalize::AlreadyIdentical => {}
                        // Another writer placed different bytes under this
                        // filename. Keep the tmp file so a retry detects the
                        // same conflict, and leave the entry out of the merge.
                        BlobFinalize::Conflict => {
                            lost_tmp_paths.push(blob.tmp_path.as_path());
                            lost_blobs.insert(blob.filename.clone());
                        }
                    }
                }
            }
            let mut claimed: HashMap<&str, Vec<&JournaledRevisionRef>> = HashMap::new();
            for revision_ref in &package.revision_refs {
                if lost_blobs.contains(&revision_ref.filename) {
                    continue;
                }
                match store
                    .write_hosted_revision_ref(
                        &revision_ref.digest,
                        &revision_ref.ref_id,
                        &self.revision_ref_owner,
                        &revision_ref.bytes,
                    )
                    .await
                {
                    Ok(
                        HostedRevisionRefWrite::Claimed | HostedRevisionRefWrite::AlreadyClaimed,
                    ) => {
                        claimed.entry(&revision_ref.filename).or_default().push(revision_ref);
                    }
                    Ok(HostedRevisionRefWrite::Committed) => {}
                    Err(RegistryError::RevisionReferenceLimit { limit }) => {
                        outcome.reference_limit = Some(limit);
                        lost_blobs.insert(revision_ref.filename.clone());
                        if let Some(claimed_refs) = claimed.remove(revision_ref.filename.as_str()) {
                            for claimed_ref in claimed_refs {
                                store
                                    .remove_hosted_revision_ref(
                                        &claimed_ref.digest,
                                        &claimed_ref.ref_id,
                                        &self.revision_ref_owner,
                                    )
                                    .await?;
                            }
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
            let journaled = fs::read(self.dir.join(&package.document_file)).await?;
            let mut written = false;
            // The journaled document was computed from `base_version` of the
            // stored one, so while the store is still there it is exactly what
            // to write — no second read, no merge. Recovery carries no base
            // version and always merges.
            if lost_blobs.is_empty()
                && let Some(base_version) = self.base_versions.get(index)
            {
                written = matches!(
                    store
                        .write_hosted_document_if_current(&name, &journaled, base_version.as_ref(),)
                        .await?,
                    DocumentWrite::Written,
                );
                if written {
                    progress.wrote_documents.insert(package.id());
                }
            }
            if !written {
                let update = store
                    .update_hosted_document_with_retry(
                        &name,
                        COMMIT_DOCUMENT_WRITE_RETRIES,
                        |existing| {
                            documents.merge(DocumentMerge {
                                ecosystem: package.ecosystem,
                                name: &name,
                                existing,
                                journaled: &journaled,
                                lost_blobs: &lost_blobs,
                            })
                        },
                    )
                    .await?;
                match update {
                    DocumentUpdate::Written => {
                        progress.wrote_documents.insert(package.id());
                    }
                    DocumentUpdate::NotFound => outcome.unrecorded.push(package.id()),
                }
            }
            for revision_ref in claimed.into_values().flatten() {
                store
                    .commit_hosted_revision_ref(
                        &revision_ref.digest,
                        &revision_ref.ref_id,
                        &self.revision_ref_owner,
                    )
                    .await?;
            }
            outcome.lost_blobs.extend(
                lost_blobs.into_iter().map(|filename| LostBlob { package: package.id(), filename }),
            );
        }
        // Remove the journal before cleaning lost tmp files so an interruption
        // cannot leave a retry that has lost the evidence needed to detect the
        // conflict.
        fs::remove_dir_all(&self.dir).await?;
        // Only clean conflict evidence after the journal removal is durable.
        let journal_removal_is_durable = match self.dir.parent() {
            Some(parent) => sync_dir(parent).await.is_ok(),
            None => false,
        };
        cleanup_lost_tmp_paths(&lost_tmp_paths, journal_removal_is_durable).await;
        Ok(())
    }
}

/// Bring the publish journal of the storage configured in `config` to a
/// consistent state. `pnpr::serve` and `pnpr::serve_listener`
/// call this before binding; embedders that build a router directly
/// should call it themselves on startup.
pub async fn recover_publish_journal(
    config: &Config,
    documents: &dyn HostedDocuments,
) -> Result<()> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    storage.publish_journal().recover(&storage, documents).await
}

async fn cleanup_lost_tmp_paths(tmp_paths: &[&Path], journal_removal_is_durable: bool) {
    if !journal_removal_is_durable {
        return;
    }
    for tmp_path in tmp_paths {
        let _ = fs::remove_file(tmp_path).await;
    }
}

/// Discard an unsealed transaction: nothing of it ever became visible,
/// so all there is to do is delete the staged tmp files it points at
/// and the journal entry itself. Errors are swallowed — this is
/// cleanup, and a leftover tmp file is harmless beyond a little disk.
async fn roll_back(dir: &Path) {
    if let Ok(bytes) = fs::read(dir.join(MANIFEST_FILE)).await
        && let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes)
    {
        for package in &manifest.packages {
            for blob in &package.blobs {
                let _ = fs::remove_file(&blob.tmp_path).await;
            }
        }
    }
    let _ = fs::remove_dir_all(dir).await;
}

/// `<zero-padded unix millis>-<pid>-<counter>`: unique per process and
/// lexically ordered by seal time across restarts.
fn txn_id() -> String {
    let millis =
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let counter = TXN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{millis:016}-{}-{counter}", std::process::id())
}

fn revision_ref_owner(dir: &Path) -> Result<&str> {
    let owner =
        dir.file_name().and_then(|name| name.to_str()).ok_or_else(|| RegistryError::Internal {
            reason: format!("publish journal path has no transaction id: {}", dir.display()),
        })?;
    if is_canonical_revision_ref_owner(owner) {
        Ok(owner)
    } else {
        Err(RegistryError::Internal {
            reason: format!("publish journal transaction id is invalid: {}", dir.display()),
        })
    }
}

async fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    Ok(())
}

#[cfg(unix)]
async fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir).await?.sync_all().await
}

#[cfg(not(unix))]
async fn sync_dir(_dir: &Path) -> io::Result<()> {
    // 표준 API로 디렉터리 엔트리의 내구성을 확인할 수 없는 플랫폼은 안전하게 미지원 처리한다.
    Err(io::Error::new(ErrorKind::Unsupported, "directory sync is not supported"))
}

#[cfg(test)]
mod tests;
