//! Crash-atomic commit journal for the publish flow.
//!
//! A publish (single-package or batch) stages every tarball into a tmp
//! file and holds the merged packuments in memory; making the result
//! visible then takes several non-atomic steps — one rename/upload per
//! tarball, one packument write per package. A crash in the middle of
//! those steps could leave some packages of a batch published and
//! others not. The journal closes that window: before anything is
//! promoted, the full intent — the merged packument bytes plus the
//! locations of the staged tmp files — is persisted under
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
    config::Config,
    error::Result,
    package_name::PackageName,
    publish::{merge_manifest, now_iso},
    storage::{Storage, TarballSlot, unique_tmp_path},
};
use serde::{Deserialize, Serialize};
use std::{
    io::ErrorKind,
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
    /// File inside the transaction directory holding the merged
    /// packument bytes.
    packument_file: String,
    tarballs: Vec<ManifestTarball>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestTarball {
    /// Canonical on-disk tarball filename (`<basename>-<version>.tgz`).
    filename: String,
    /// The staged tmp file holding the verified tarball bytes.
    tmp_path: PathBuf,
}

/// One package of a publish about to be committed, borrowed from the
/// handler's staged state.
pub struct JournaledPublish<'a> {
    pub name: &'a PackageName,
    pub packument: &'a [u8],
    pub slots: &'a [TarballSlot],
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
        let dir = self.root.join(txn_id());
        fs::create_dir_all(&dir).await?;
        let mut manifest = Manifest { packages: Vec::with_capacity(packages.len()) };
        for (index, package) in packages.iter().enumerate() {
            let packument_file = format!("packument-{index}.json");
            write_synced(&dir.join(&packument_file), package.packument).await?;
            manifest.packages.push(ManifestPackage {
                name: package.name.as_str().to_string(),
                packument_file,
                tarballs: package
                    .slots
                    .iter()
                    .map(|slot| ManifestTarball {
                        filename: slot.filename().to_string(),
                        tmp_path: slot.tmp_path.clone(),
                    })
                    .collect(),
            });
        }
        write_synced(&dir.join(MANIFEST_FILE), &serde_json::to_vec_pretty(&manifest)?).await?;
        sync_dir(&dir).await;
        // The seal itself: a single same-directory rename, atomic on
        // POSIX. Recovery treats a directory without this marker as an
        // aborted transaction and rolls it back.
        let marker = dir.join(COMMIT_MARKER);
        let marker_tmp = unique_tmp_path(&marker);
        write_synced(&marker_tmp, b"").await?;
        fs::rename(&marker_tmp, &marker).await?;
        sync_dir(&dir).await;
        Ok(SealedTxn { dir })
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
                roll_forward(storage, &dir).await?;
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
    /// Apply the sealed transaction now, completing any apply steps that
    /// have not run yet, and remove the journal entry. This is the same
    /// idempotent roll-forward startup recovery performs; commit calls it
    /// to self-heal when the inline apply fails partway, so a running
    /// server never leaves a sealed batch partially visible until the
    /// next restart.
    pub async fn roll_forward(self, storage: &Storage) -> Result<()> {
        roll_forward(storage, &self.dir).await
    }

    /// Remove the journal entry once the publish is fully applied.
    /// Best-effort: a leftover sealed entry is simply re-applied
    /// (idempotently) by the next startup recovery.
    pub async fn finish(self) {
        let _ = fs::remove_dir_all(&self.dir).await;
    }
}

/// Roll the publish journal of the storage configured in `config` to a
/// consistent state. [`crate::serve`] and [`crate::serve_listener`]
/// call this before binding; embedders that build a router directly
/// should call it themselves on startup.
pub async fn recover_publish_journal(config: &Config) -> Result<()> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone());
    storage.publish_journal().recover(&storage).await
}

/// Re-apply a sealed transaction. Every step tolerates having already
/// run before the crash: a tmp file that's gone was already promoted,
/// and the packument is re-merged into the current on-disk state
/// instead of overwriting it.
async fn roll_forward(storage: &Storage, dir: &Path) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join(MANIFEST_FILE)).await?)?;
    for package in &manifest.packages {
        let name = PackageName::parse(&package.name)?;
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
                storage.finalize_tarball_slot(slot).await?;
            }
        }
        let journaled: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(&package.packument_file)).await?)?;
        let existing: Option<serde_json::Value> = match storage.read_hosted_packument(&name).await?
        {
            Some(bytes) => Some(serde_json::from_slice(&bytes)?),
            None => None,
        };
        let merged = merge_manifest(existing.as_ref(), &journaled, &now_iso());
        storage.write_hosted_packument(&name, &serde_json::to_vec_pretty(&merged)?).await?;
    }
    fs::remove_dir_all(dir).await?;
    Ok(())
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

async fn write_synced(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    Ok(())
}

/// Flush directory entries (file creations and renames) to disk.
/// Best-effort: directory fsync isn't available everywhere (Windows
/// can't open directories as files), and a lost dir entry only widens
/// the crash window back to what it was without the journal.
async fn sync_dir(dir: &Path) {
    if cfg!(unix)
        && let Ok(file) = fs::File::open(dir).await
    {
        let _ = file.sync_all().await;
    }
}
