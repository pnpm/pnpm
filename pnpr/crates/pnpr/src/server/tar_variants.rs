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
    io::{self, Read, Write},
    path::PathBuf,
    sync::OnceLock,
};
use tokio::sync::mpsc;

/// Cache namespace the variants, and the tar copies they are made from, live in.
const NAMESPACE: &str = "~tar-zstd19";

/// Copies waiting for the worker. A copy offered to a full queue is dropped,
/// and the version's next zstd request offers it again.
const QUEUE_DEPTH: usize = 32;

/// Tars larger than this keep the level-3 encoding: the worker holds a tar and
/// its variant in memory while it compresses.
const MAX_VARIANT_TAR_BYTES: u64 = 256 * 1024 * 1024;

const LEVEL: i32 = 19;

/// The largest window `Content-Encoding: zstd` allows (RFC 9659), as `log2`.
const WINDOW_LOG: u32 = 23;

/// The background worker's queue, started on first use so the server needs
/// no runtime to be constructed.
pub(super) struct TarVariantQueue {
    sender: OnceLock<mpsc::Sender<VariantJob>>,
}

struct VariantJob {
    storage: Storage,
    name: CanonicalPackageName,
    filename: String,
    tar_path: PathBuf,
}

impl TarVariantQueue {
    pub(super) fn new() -> Self {
        Self { sender: OnceLock::new() }
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
    written: u64,
    job: VariantJob,
    sender: mpsc::Sender<VariantJob>,
}

impl TarCopy {
    /// Start a copy for the tar of the `.tgz` with integrity `tgz_integrity`.
    pub(super) async fn start(
        queue: &TarVariantQueue,
        storage: &Storage,
        name: &CanonicalPackageName,
        tgz_integrity: &str,
    ) -> Option<Self> {
        let copy_name = format!("{}.copy", sha256_hex(tgz_integrity.as_bytes()));
        let write = storage.open_upstream_blob_tmp(NAMESPACE, name, &copy_name).await.ok()?;
        let (file, _, tar_path) = write.into_temp_file().await.ok()?;
        let job = VariantJob {
            storage: storage.clone(),
            name: name.clone(),
            filename: variant_filename(tgz_integrity),
            tar_path,
        };
        Some(Self { file: file.into_std().await, written: 0, job, sender: queue.sender() })
    }

    /// Append decoded bytes. `None` once the copy is abandoned: it grew past
    /// [`MAX_VARIANT_TAR_BYTES`] or failed to write.
    pub(super) fn write(mut self, bytes: &[u8]) -> Option<Self> {
        self.written = self.written.saturating_add(bytes.len() as u64);
        if self.written > MAX_VARIANT_TAR_BYTES || self.file.write_all(bytes).is_err() {
            self.discard();
            return None;
        }
        Some(self)
    }

    /// Offer the complete copy to the worker.
    pub(super) fn submit(self) {
        let Self { file, job, sender, .. } = self;
        drop(file);
        if let Err(rejected) = sender.try_send(job) {
            let job = match rejected {
                mpsc::error::TrySendError::Full(job) | mpsc::error::TrySendError::Closed(job) => {
                    job
                }
            };
            remove_copy(&job.tar_path);
        }
    }

    pub(super) fn discard(self) {
        remove_copy(&self.job.tar_path);
    }
}

fn remove_copy(path: &std::path::Path) {
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
        remove_copy(&job.tar_path);
    }
}

async fn write_variant(job: &VariantJob) -> io::Result<()> {
    if job.storage
        .open_upstream_blob(NAMESPACE, &job.name, &job.filename)
        .await
        .is_ok_and(|found| found.is_some())
    {
        return Ok(());
    }
    let tar_path = job.tar_path.clone();
    let encoded = tokio::task::spawn_blocking(move || encode_level_19(&tar_path))
        .await
        .map_err(io::Error::other)??;
    let mut variant = job.storage
        .open_upstream_blob_tmp(NAMESPACE, &job.name, &job.filename)
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
