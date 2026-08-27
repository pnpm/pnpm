//! Crash-atomic commit journal for the publish flow.
//!
//! A publish (single-package or batch) stages every tarball into a tmp
//! file and holds the merged packuments in memory; making the result
//! visible then takes several non-atomic steps — one rename/upload per
//! tarball, one packument write per package. A crash in the middle of
//! those steps could leave some packages of a batch published and
//! others not. The journal closes that window: before anything is
//! promoted, the full intent — the merged packument bytes, revision
//! references, and locations of the staged tmp files — is persisted under
//! `.pnpr-journal/<txn>/` and sealed with a single atomic rename of the
//! `commit` marker. [`recover_publish_journal`] runs at startup, before
//! the server accepts requests: sealed transactions are rolled forward
//! (every apply step is idempotent) and unsealed ones are rolled back,
//! so a publish is either fully visible or fully absent.
//!
//! Once a transaction is sealed, the publish *will* become visible —
//! if applying it fails at request time (e.g. the S3 backend is briefly
//! unreachable), the client sees an error but the sealed transaction
//! completes on the next startup. An operator can abort a sealed-but-
//! unapplied transaction by deleting its directory.
//!
//! Roll-forward re-merges the journaled packument into whatever is on
//! disk at recovery time (rather than overwriting), so replaying an old
//! sealed transaction cannot erase versions published between the
//! failed apply and the restart.

use crate::{
    HostedRevisionRefWrite, RECOVERY_PACKUMENT_WRITE_RETRIES, Storage, TarballFinalize,
    TarballSlot, is_canonical_revision_ref_owner,
    publish::{merge_manifest, now_iso},
    unique_tmp_path,
};
use pnpr_config::Config;
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::PackageName;
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
    /// Hosted-org storage namespace this package publishes into, or `None` for
    /// the flat (path-less) hosted store. Recovery namespaces the roll-forward
    /// by it so a crash mid-commit promotes into the right org. Defaulted for
    /// back-compat with journals written before org registries existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    org: Option<String>,
    /// File inside the transaction directory holding the merged
    /// packument bytes.
    packument_file: String,
    tarballs: Vec<ManifestTarball>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revision_refs: Vec<JournaledRevisionRef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestTarball {
    /// Canonical on-disk tarball filename (`<basename>-<version>.tgz`).
    filename: String,
    /// The staged tmp file holding the verified tarball bytes.
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
pub struct JournaledPublish<'a> {
    pub name: &'a PackageName,
    /// Hosted-org storage namespace, or `None` for the flat hosted store.
    pub org: Option<&'a str>,
    pub packument: &'a [u8],
    pub slots: &'a [TarballSlot],
    pub revision_refs: &'a [JournaledRevisionRef],
}

/// Handle to the journal directory of one [`Storage`].
pub struct PublishJournal {
    root: PathBuf,
}

/// A sealed transaction: the journal entry is durable and carries the
/// commit marker. Call [`Self::finish`] after the publish is fully
/// applied; dropping it without finishing just leaves the entry for
/// startup recovery to (idempotently) re-apply.
pub struct SealedTxn {
    dir: PathBuf,
    revision_ref_owner: String,
}

impl PublishJournal {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Persist the full intent of the publish and seal it with the
    /// commit marker. After this returns `Ok`, the publish is
    /// committed: either the caller applies it now, or startup
    /// recovery does.
    pub async fn seal(&self, packages: &[JournaledPublish<'_>]) -> Result<SealedTxn> {
        let revision_ref_owner = txn_id();
        let dir = self.root.join(&revision_ref_owner);
        fs::create_dir_all(&dir).await?;
        let mut manifest = Manifest { packages: Vec::with_capacity(packages.len()) };
        for (index, package) in packages.iter().enumerate() {
            let packument_file = format!("packument-{index}.json");
            write_synced(&dir.join(&packument_file), package.packument).await?;
            manifest.packages.push(ManifestPackage {
                name: package.name.as_str().to_string(),
                org: package.org.map(str::to_string),
                packument_file,
                tarballs: package
                    .slots
                    .iter()
                    .map(|slot| ManifestTarball {
                        filename: slot.filename().to_string(),
                        tmp_path: slot.tmp_path.clone(),
                    })
                    .collect(),
                revision_refs: package.revision_refs.to_vec(),
            });
        }
        write_synced(&dir.join(MANIFEST_FILE), &serde_json::to_vec_pretty(&manifest)?).await?;
        let _ = sync_dir(&dir).await;
        // The seal itself: a single same-directory rename, atomic on
        // POSIX. Recovery treats a directory without this marker as an
        // aborted transaction and rolls it back.
        let marker = dir.join(COMMIT_MARKER);
        let marker_tmp = unique_tmp_path(&marker);
        write_synced(&marker_tmp, b"").await?;
        fs::rename(&marker_tmp, &marker).await?;
        let _ = sync_dir(&dir).await;
        Ok(SealedTxn { dir, revision_ref_owner })
    }

    /// Roll every journal entry to a consistent state: sealed
    /// transactions forward, unsealed ones back. Must run before the
    /// server accepts requests — it takes no package locks.
    pub async fn recover(&self, storage: &Storage) -> Result<()> {
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
                let revision_ref_owner = revision_ref_owner(&dir)?;
                roll_forward(storage, &dir, revision_ref_owner).await?;
                tracing::info!(txn = %dir.display(), "rolled publish journal entry forward");
            } else {
                roll_back(&dir).await;
                tracing::info!(txn = %dir.display(), "rolled publish journal entry back");
            }
        }
        Ok(())
    }
}

impl SealedTxn {
    #[must_use]
    pub fn revision_ref_owner(&self) -> &str {
        &self.revision_ref_owner
    }

    /// Apply the sealed transaction now, completing any apply steps that
    /// have not run yet, and remove the journal entry. This is the same
    /// idempotent roll-forward startup recovery performs; commit calls it
    /// to self-heal when the inline apply fails partway, so a running
    /// server never leaves a sealed batch partially visible until the
    /// next restart.
    pub async fn roll_forward(self, storage: &Storage) -> Result<()> {
        roll_forward(storage, &self.dir, &self.revision_ref_owner).await
    }

    /// Remove the journal entry once the publish is fully applied.
    /// Best-effort: a leftover sealed entry is simply re-applied
    /// (idempotently) by the next startup recovery.
    pub async fn finish(self) {
        let _ = fs::remove_dir_all(&self.dir).await;
    }
}

/// Roll the publish journal of the storage configured in `config` to a
/// consistent state. `pnpr::serve` and `pnpr::serve_listener`
/// call this before binding; embedders that build a router directly
/// should call it themselves on startup.
pub async fn recover_publish_journal(config: &Config) -> Result<()> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    storage.publish_journal().recover(&storage).await
}

/// Re-apply a sealed transaction. Every step tolerates having already
/// run before the crash: a tmp file that's gone was already promoted,
/// and the packument is re-merged into the current on-disk state
/// instead of overwriting it.
async fn roll_forward(storage: &Storage, dir: &Path, revision_ref_owner: &str) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join(MANIFEST_FILE)).await?)?;
    let mut conflicted_tmp_paths = Vec::new();
    for package in &manifest.packages {
        let name = PackageName::parse(&package.name)?;
        // Roll forward into the package's hosted namespace (or the flat
        // store when it has none), so a crash mid-commit promotes the staged
        // tarballs and packument into exactly the store the publish targeted.
        let store = match &package.org {
            Some(org) => storage.for_hosted(org),
            None => storage.clone(),
        };
        let mut conflicted_versions = HashSet::new();
        for tarball in &package.tarballs {
            // A missing tmp file was already promoted before the crash, so
            // skip it. But never read an I/O error as "missing": that would
            // skip promotion, write the packument anyway, and delete the
            // journal entry — advertising a tarball with nothing on disk and
            // no journal state left to retry from. Propagate it instead so
            // recovery aborts and the entry survives for a later attempt.
            if fs::try_exists(&tarball.tmp_path).await? {
                let slot = TarballSlot::from_parts(
                    tarball.tmp_path.clone(),
                    name.clone(),
                    tarball.filename.clone(),
                );
                match store.finalize_tarball_slot(slot).await? {
                    TarballFinalize::Written | TarballFinalize::AlreadyIdentical => {}
                    // Another replica finalized different bytes for this version.
                    // Keep the tmp file so a retry detects the same conflict, and
                    // exclude the losing version from the merge below.
                    TarballFinalize::Conflict => {
                        conflicted_tmp_paths.push(tarball.tmp_path.as_path());
                        let (_, version) = name.parse_tarball_name(&tarball.filename)?;
                        conflicted_versions.insert(version);
                    }
                }
            }
        }
        let mut journaled: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(&package.packument_file)).await?)?;
        let revision_refs = package
            .revision_refs
            .iter()
            .map(|revision_ref| {
                let (_, version) = name.parse_tarball_name(&revision_ref.filename)?;
                Ok((revision_ref, version))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut applied_revision_refs: HashMap<String, Vec<&JournaledRevisionRef>> = HashMap::new();
        for (revision_ref, version) in revision_refs {
            if conflicted_versions.contains(&version) {
                continue;
            }
            match store
                .write_hosted_revision_ref(
                    &revision_ref.digest,
                    &revision_ref.ref_id,
                    revision_ref_owner,
                    &revision_ref.bytes,
                )
                .await
            {
                Ok(HostedRevisionRefWrite::Claimed | HostedRevisionRefWrite::AlreadyClaimed) => {
                    applied_revision_refs.entry(version).or_default().push(revision_ref);
                }
                Ok(HostedRevisionRefWrite::Committed) => {}
                Err(pnpr_error::RegistryError::RevisionReferenceLimit { .. }) => {
                    conflicted_versions.insert(version.clone());
                    if let Some(applied) = applied_revision_refs.remove(&version) {
                        for applied_ref in applied {
                            store
                                .remove_hosted_revision_ref(
                                    &applied_ref.digest,
                                    &applied_ref.ref_id,
                                    revision_ref_owner,
                                )
                                .await?;
                        }
                    }
                }
                Err(err) => return Err(err),
            }
        }
        if !conflicted_versions.is_empty() {
            drop_conflicted_versions(&mut journaled, &conflicted_versions);
        }
        write_merged_packument(&store, &name, &journaled).await?;
        for revision_ref in applied_revision_refs.into_values().flatten() {
            store
                .commit_hosted_revision_ref(
                    &revision_ref.digest,
                    &revision_ref.ref_id,
                    revision_ref_owner,
                )
                .await?;
        }
    }
    // Remove the journal before cleaning conflicted tmp files so an interruption
    // cannot leave a retry that has lost the evidence needed to detect conflict.
    fs::remove_dir_all(dir).await?;
    // Only clean conflict evidence after the journal removal is durable.
    let journal_removal_is_durable = match dir.parent() {
        Some(parent) => sync_dir(parent).await.is_ok(),
        None => false,
    };
    cleanup_conflicted_tmp_paths(&conflicted_tmp_paths, journal_removal_is_durable).await;
    Ok(())
}

async fn cleanup_conflicted_tmp_paths(tmp_paths: &[&Path], journal_removal_is_durable: bool) {
    if !journal_removal_is_durable {
        return;
    }
    for tmp_path in tmp_paths {
        let _ = fs::remove_file(tmp_path).await;
    }
}

async fn write_merged_packument(
    store: &Storage,
    name: &PackageName,
    journaled: &serde_json::Value,
) -> Result<()> {
    store
        .update_hosted_packument_with_retry(
            name,
            RECOVERY_PACKUMENT_WRITE_RETRIES,
            |existing_bytes| {
                let existing: Option<serde_json::Value> = match existing_bytes {
                    Some(bytes) => Some(serde_json::from_slice(bytes)?),
                    None => None,
                };
                let merged =
                    merge_manifest(existing.as_ref(), journaled, existing.as_ref(), &now_iso());
                Ok(Some(serde_json::to_vec_pretty(&merged)?))
            },
        )
        .await?;
    Ok(())
}

/// Drop from a journaled manifest every version that lost an immutable tarball
/// slot or could not reserve a bounded digest-reference slot. The journal keeps
/// each staged attachment's canonical filename, so callers resolve that name to
/// the version before reaching this helper instead of trusting a publisher-
/// supplied `dist.tarball` URL as the transaction identity.
fn drop_conflicted_versions(journaled: &mut serde_json::Value, conflicted: &HashSet<String>) {
    let Some(versions) = journaled.get_mut("versions").and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    let mut removed_versions = HashSet::new();
    versions.retain(|version, _| {
        let keep = !conflicted.contains(version);
        if !keep {
            removed_versions.insert(version.clone());
        }
        keep
    });

    if let Some(tags) = journaled.get_mut("dist-tags").and_then(serde_json::Value::as_object_mut) {
        tags.retain(|_, version| {
            version.as_str().is_none_or(|version| !removed_versions.contains(version))
        });
    }
    if let Some(time) = journaled.get_mut("time").and_then(serde_json::Value::as_object_mut) {
        time.retain(|version, _| !removed_versions.contains(version));
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
            for tarball in &package.tarballs {
                let _ = fs::remove_file(&tarball.tmp_path).await;
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
