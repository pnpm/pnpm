//! Level-19 zstd variants of tar responses.
//!
//! A zstd response is compressed at level 3 as it streams until a stored
//! level-19 variant replaces it. The first such response keeps a copy of the
//! tar it decoded and offers it to one background worker, which writes the
//! variant into the replica-local cache. A variant is disposable: no hash
//! depends on its bytes, so an evicted one only means level 3 again until a
//! later request regenerates it.

use super::{CanonicalPackageName, ecosystem::sha256_hex};
use pnpr_storage::Storage;
use std::{
    collections::HashSet,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc;

/// Cache namespace the variants, and the tar copies they are made from, live in.
pub(crate) const NAMESPACE: &str = "~tar-zstd19";

/// Copies waiting for the worker. A copy offered to a full queue is dropped,
/// and the version's next zstd request offers it again.
const QUEUE_DEPTH: usize = 32;

/// Tars larger than this keep the level-3 encoding: the worker holds a tar and
/// its variant in memory while it compresses.
const MAX_VARIANT_TAR_BYTES: u64 = 256 * 1024 * 1024;

/// Ceiling on the bytes all tar copies waiting for, or held by, the worker may
/// take together. A copy reserves each chunk before writing it and is dropped
/// when the quota has no room.
const COPY_QUOTA_BYTES: u64 = 1024 * 1024 * 1024;

const LEVEL: i32 = 19;

/// The largest window `Content-Encoding: zstd` allows (RFC 9659), as `log2`.
const WINDOW_LOG: u32 = 23;

/// The background worker's queue, started on first use so the server needs
/// no runtime to be constructed.
pub(super) struct TarVariantQueue {
    sender: OnceLock<mpsc::Sender<VariantJob>>,
    copies: Arc<CopyQuota>,
}

/// What the tar copies in flight hold: their bytes, and the variants they are
/// for, so concurrent misses for one version share a single copy.
#[derive(Default)]
struct CopyQuota {
    reserved: AtomicU64,
    variants: Mutex<HashSet<String>>,
}

/// One copy's share of the [`CopyQuota`], and its file. Dropping it removes
/// the file and returns both.
struct CopyReservation {
    quota: Arc<CopyQuota>,
    variant: String,
    bytes: u64,
    tar_path: PathBuf,
}

impl CopyReservation {
    fn reserve(&mut self, bytes: u64) -> bool {
        let total = self.bytes.saturating_add(bytes);
        if total > MAX_VARIANT_TAR_BYTES {
            return false;
        }
        let granted =
            self.quota.reserved.try_update(Ordering::AcqRel, Ordering::Acquire, |reserved| {
                reserved
                    .checked_add(bytes)
                    .filter(|reserved| *reserved <= COPY_QUOTA_BYTES)
            });
        if granted.is_ok() {
            self.bytes = total;
        }
        granted.is_ok()
    }
}

impl Drop for CopyReservation {
    fn drop(&mut self) {
        remove_copy(&self.tar_path);
        self.quota.reserved.fetch_sub(self.bytes, Ordering::AcqRel);
        self.quota.variants
            .lock()
            .expect("copy quota lock poisoned")
            .remove(&self.variant);
    }
}

struct VariantJob {
    storage: Storage,
    name: CanonicalPackageName,
    copy: CopyReservation,
}

impl TarVariantQueue {
    pub(super) fn new() -> Self {
        Self { sender: OnceLock::new(), copies: Arc::default() }
    }

    fn sender(&self) -> mpsc::Sender<VariantJob> {
        self.sender
            .get_or_init(|| {
                let (sender, receiver) = mpsc::channel(QUEUE_DEPTH);
                tokio::spawn(run_worker(receiver));
                sender
            })
            .clone()
    }

    /// Claim the copy for `variant`, unless another request is making it.
    fn claim(&self, variant: &str) -> bool {
        self.copies.variants
            .lock()
            .expect("copy quota lock poisoned")
            .insert(variant.to_string())
    }
}

/// The stored variant of the tar of the `.tgz` with integrity `tgz_integrity`.
pub(super) async fn open_variant(
    storage: &Storage,
    name: &CanonicalPackageName,
    tgz_integrity: &str,
) -> Option<(tokio::fs::File, u64)> {
    let filename = variant_filename(tgz_integrity);
    storage
        .open_upstream_blob(NAMESPACE, name, &filename)
        .await
        .ok()
        .flatten()
}

/// A copy of a tar being decoded, written on the decoder's blocking thread
/// and offered to the worker once the decode succeeded.
pub(super) struct TarCopy {
    file: std::fs::File,
    job: VariantJob,
    sender: mpsc::Sender<VariantJob>,
}

impl TarCopy {
    /// Start a copy for the tar of the `.tgz` with integrity `tgz_integrity`.
    /// `None` when another request is already copying it.
    pub(super) async fn start(
        queue: &TarVariantQueue,
        storage: &Storage,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
    ) -> Option<Self> {
        Self::start_with_sender(queue, storage, name, tgz_integrity, queue.sender()).await
    }

    async fn start_with_sender(
        queue: &TarVariantQueue,
        storage: &Storage,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
        sender: mpsc::Sender<VariantJob>,
    ) -> Option<Self> {
        let variant = variant_filename(tgz_integrity);
        if !queue.claim(&variant) {
            return None;
        }
        let mut copy = CopyReservation {
            quota: Arc::clone(&queue.copies),
            variant,
            bytes: 0,
            tar_path: PathBuf::new(),
        };
        let copy_name = format!("{}.copy", sha256_hex(tgz_integrity.as_bytes()));
        let write = storage.open_upstream_blob_tmp(NAMESPACE, name, &copy_name).await.ok()?;
        let (file, _, tar_path) = write.into_temp_file().await.ok()?;
        copy.tar_path = tar_path;
        let job = VariantJob { storage: storage.clone(), name: name.clone(), copy };
        Some(Self { file: file.into_std().await, job, sender })
    }

    /// Append decoded bytes. `None` once the copy is dropped: it outgrew
    /// [`MAX_VARIANT_TAR_BYTES`] or the [`COPY_QUOTA_BYTES`], or failed to
    /// write.
    pub(super) fn write(mut self, bytes: &[u8]) -> Option<Self> {
        let fits = self.job.copy.reserve(bytes.len() as u64);
        (fits && self.file.write_all(bytes).is_ok()).then_some(self)
    }

    /// Offer the complete copy to the worker. A full queue drops it.
    pub(super) fn submit(self) {
        let Self { file, job, sender } = self;
        drop(file);
        let _ = sender.try_send(job);
    }
}

fn remove_copy(path: &std::path::Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    if let Err(err) = std::fs::remove_file(path)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(?err, path = %path.display(), "removing a tar copy failed");
    }
}

fn variant_filename(tgz_integrity: &str) -> String {
    format!("{}.tar.zst", sha256_hex(tgz_integrity.as_bytes()))
}

async fn run_worker(mut receiver: mpsc::Receiver<VariantJob>) {
    while let Some(job) = receiver.recv().await {
        if let Err(err) = write_variant(&job).await {
            tracing::warn!(?err, package = %job.name.as_str(), "writing a tar variant failed");
        }
    }
}

async fn write_variant(job: &VariantJob) -> io::Result<()> {
    if job.storage
        .open_upstream_blob(NAMESPACE, &job.name, &job.copy.variant)
        .await
        .is_ok_and(|found| found.is_some())
    {
        return Ok(());
    }
    let tar_path = job.copy.tar_path.clone();
    let encoded = tokio::task::spawn_blocking(move || encode_level_19(&tar_path))
        .await
        .map_err(io::Error::other)??;
    let mut variant = job.storage
        .open_upstream_blob_tmp(NAMESPACE, &job.name, &job.copy.variant)
        .await
        .map_err(io::Error::other)?;
    variant.write_all(&encoded).await?;
    variant.finalize().await
}

fn encode_level_19(tar_path: &std::path::Path) -> io::Result<Vec<u8>> {
    let mut tar = Vec::new();
    std::fs::File::open(tar_path)?.read_to_end(&mut tar)?;
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), LEVEL)?;
    encoder.window_log(WINDOW_LOG)?;
    encoder.include_contentsize(true)?;
    encoder.set_pledged_src_size(Some(tar.len() as u64))?;
    encoder.write_all(&tar)?;
    encoder.finish()
}

#[cfg(test)]
mod tests;
