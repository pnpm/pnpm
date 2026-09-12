use super::{
    Arc, AtomicBool, CafsFileInfo, HashMap, Ordering, PackageFilesIndex, SideEffectsDiff, StoreDir,
    StoreIndex, StoreIndexError,
};

/// Handle producers use to hand rows off to the batched writer task. Clone
/// cheaply via [`Arc`] to share across tokio tasks.
///
/// The design follows a queue-and-flush pattern: producers don't touch
/// `SQLite`, they just push `(key, value)` onto an unbounded channel. A single
/// [`spawn_blocking`][tokio::task::spawn_blocking] task drains the channel,
/// collects each non-blocking burst into a batch (capped at 256 entries —
/// see `MAX_BATCH_SIZE`), and flushes it with one `BEGIN IMMEDIATE` ...
/// `COMMIT`. That turns the per-snapshot `Connection::open` + 7-PRAGMA +
/// solo-INSERT pattern into one open + N transactions, amortizes the WAL
/// commit fsync across the batch, and leaves tokio's blocking pool alone
/// (one writer thread, not one per tarball).
pub struct StoreIndexWriter {
    tx: tokio::sync::mpsc::UnboundedSender<WriteMsg>,
    /// One-shot log guard for the "channel closed" case in [`Self::queue`].
    /// A dead writer (task panicked, [`StoreIndex::open`] failed) means
    /// every subsequent `queue` call fails — without this guard that
    /// spams 1352+ identical warnings into the install log. Once the
    /// first failure has been logged, further failures go silent;
    /// subsequent installs will still observe the missing index rows
    /// and re-download, which is the only actionable signal anyway.
    pub(super) warn_on_send_failure: AtomicBool,
}

/// Messages the writer task processes in arrival order. Coalesced
/// per-`key` inside each batch so multiple mutations to the same
/// store-index row apply against the same in-memory [`PackageFilesIndex`]
/// before the batch's `INSERT OR REPLACE` flush.
enum WriteMsg {
    /// Wholesale replace the row at `key`. Used by the prefetch /
    /// download path that already has the full [`PackageFilesIndex`]
    /// in hand.
    Replace {
        key: String,
        value: PackageFilesIndex,
    },
    /// Read-modify-write the row at `key`: load the existing row
    /// (from the same writer task's pending state or, on a miss,
    /// from `SQLite`), compute the diff between `current_files` and
    /// the row's `files`, insert `(cache_key → diff)` into
    /// `side_effects`, and re-queue the row. Used by
    /// [`crate::upload()`] to seed the side-effects cache after a
    /// successful postinstall.
    ///
    /// Routing R/M/W through the writer task is the simplest way
    /// to make multi-snapshot uploads against the same row
    /// commutative — every read happens after every previously-
    /// queued write for the same key (committed in this batch or
    /// in a prior one). Doing it on the caller thread with a
    /// readonly handle would let a second upload re-read the
    /// pre-mutation state and overwrite the first.
    SideEffectsUpload {
        key: String,
        cache_key: String,
        current_files: HashMap<String, CafsFileInfo>,
        response: Option<std::sync::mpsc::SyncSender<Option<SideEffectsDiff>>>,
    },
    RemoteSideEffects {
        key: String,
        cache_key: String,
        diff: SideEffectsDiff,
    },
    QuarantineRemoteSideEffects {
        key: String,
        channel: String,
        envelope_digest: String,
    },
}

const MAX_QUARANTINED_REMOTE_SIDE_EFFECTS: usize = 64;

/// Batch cap for [`StoreIndexWriter`]. Big enough that a 1352-snapshot
/// install flushes in a handful of transactions (so the fsync cost is
/// amortized), small enough that a single failing row doesn't cost
/// thousands of predecessors' worth of redo work on rollback.
const MAX_BATCH_SIZE: usize = 256;

impl StoreIndexWriter {
    /// Spawn the batched writer task. Returns the handle producers push
    /// rows to, and a [`JoinHandle`][tokio::task::JoinHandle] the caller
    /// must `await` after dropping the last `Arc` to the handle so the
    /// final batch flushes before the install returns.
    ///
    /// The writer task owns the [`StoreIndex`] connection for its entire
    /// lifetime; on DB open failure the task returns the error and the
    /// channel closes on the first producer send.
    pub fn spawn(
        store_dir: &StoreDir,
    ) -> (Arc<StoreIndexWriter>, tokio::task::JoinHandle<Result<(), StoreIndexError>>) {
        let store_root = store_dir.root().to_path_buf();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WriteMsg>();
        let handle = tokio::task::spawn_blocking(move || -> Result<(), StoreIndexError> {
            let mut index = StoreIndex::open(&store_root)?;
            let mut batch: Vec<WriteMsg> = Vec::with_capacity(MAX_BATCH_SIZE);
            while let Some(first) = rx.blocking_recv() {
                batch.push(first);
                drain_queued(&mut rx, &mut batch);
                flush_batch(&mut index, &mut batch);
            }
            Ok(())
        });
        (Arc::new(StoreIndexWriter { tx, warn_on_send_failure: AtomicBool::new(true) }), handle)
    }

    /// [`StoreIndexWriter::spawn`], or [`StoreIndexWriter::spawn_disabled`]
    /// under `frozenStore` where the store is read-only and there is
    /// nothing to write back.
    #[must_use]
    pub fn spawn_for(
        store_dir: &StoreDir,
        frozen_store: bool,
    ) -> (Arc<StoreIndexWriter>, tokio::task::JoinHandle<Result<(), StoreIndexError>>) {
        if frozen_store {
            StoreIndexWriter::spawn_disabled()
        } else {
            StoreIndexWriter::spawn(store_dir)
        }
    }

    /// Wait for the final batch flush after the last handle has been
    /// dropped.
    ///
    /// Errors are warned rather than propagated: by this point the work
    /// the rows describe is already on disk, so a missed index write
    /// only costs a re-fetch on the next install. `outcome` is appended
    /// to the log line to name the path that drained.
    pub async fn drain(
        writer_task: tokio::task::JoinHandle<Result<(), StoreIndexError>>,
        outcome: &str,
    ) {
        match writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(
                target: "pacquet::store_index",
                ?error,
                "store-index writer task returned an error{}", outcome,
            ),
            Err(error) => tracing::warn!(
                target: "pacquet::store_index",
                ?error,
                "store-index writer task panicked{}", outcome,
            ),
        }
    }

    /// Spawn a writer that never opens `index.db` — it drains every
    /// queued message and drops it.
    ///
    /// Used when `frozenStore` is enabled: the store is opened read-only,
    /// so there is nothing to write back. Producers keep queuing rows
    /// through the normal handle (the call sites stay identical), but the
    /// task touches no `SQLite` connection, so no `index.db` / WAL / SHM
    /// sidecar is opened or created under the read-only store root.
    ///
    /// Returns the same `(handle, JoinHandle)` shape as [`Self::spawn`] so
    /// call sites can branch on `frozen_store` without diverging.
    #[must_use]
    pub fn spawn_disabled()
    -> (Arc<StoreIndexWriter>, tokio::task::JoinHandle<Result<(), StoreIndexError>>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WriteMsg>();
        let handle = tokio::spawn(async move {
            while rx.recv().await.is_some() {}
            Ok::<(), StoreIndexError>(())
        });
        (Arc::new(StoreIndexWriter { tx, warn_on_send_failure: AtomicBool::new(true) }), handle)
    }
}

/// Fold one queued [`WriteMsg`] into the batch's in-flight
/// `pending` map. Pure function on `(&mut StoreIndex, &mut
/// HashMap, WriteMsg)` so the writer-task closure stays a thin
/// drain loop; correctness of each variant lives here.
///
/// Drain whatever else is already queued to maximize batch size without ever
/// blocking on the channel — a single `recv` by the caller is the only
/// blocking wait per transaction. Capped at [`MAX_BATCH_SIZE`] so a producer
/// storm doesn't grow an unbounded buffer on the writer side.
fn drain_queued(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<WriteMsg>,
    batch: &mut Vec<WriteMsg>,
) {
    while batch.len() < MAX_BATCH_SIZE {
        // `Empty` / `Disconnected` both mean "nothing more to drain right
        // now" — the caller flushes the current batch and loops back; if the
        // channel is disconnected its `blocking_recv` returns `None` next and
        // the task exits cleanly.
        let Ok(item) = rx.try_recv() else { break };
        batch.push(item);
    }
}

/// Coalesce the batch by key and write it in one transaction.
///
/// Multiple writes for the same store-index row arriving in the same batch
/// get applied in order against a single in-memory [`PackageFilesIndex`]
/// value, which then flushes once. This is what makes two
/// `SideEffectsUpload`s for the same row commutative: each builds on the
/// previous one's mutation rather than re-reading the pre-batch state from
/// `SQLite`.
fn flush_batch(index: &mut StoreIndex, batch: &mut Vec<WriteMsg>) {
    let mut pending: HashMap<String, PackageFilesIndex> = HashMap::with_capacity(batch.len());
    for msg in batch.drain(..) {
        apply_write_msg(index, &mut pending, msg);
    }
    if let Err(error) = index.set_many(pending.drain()) {
        // Drop the batch and keep going. One failed flush (e.g. a disk-full
        // hiccup) shouldn't silently drop the rest of the install's entries;
        // the next install will cache-miss those rows and re-populate them,
        // matching the "best-effort index" stance the read path already
        // takes.
        tracing::warn!(
            target: "pacquet::store_index",
            ?error,
            "batched store-index write failed; dropping this batch and continuing",
        );
    }
}

/// `Replace` is a straight overwrite. `SideEffectsUpload` does
/// the read-modify-write: it loads the row from `pending` (if a
/// prior message in this batch already touched it) or from
/// `SQLite`, then layers the diff on top. Three short-circuit
/// branches log and skip without removing the row from
/// `pending`, so a same-batch `Replace` for the same key still
/// flushes — see the docs on
/// [`WriteMsg::SideEffectsUpload`].
fn apply_write_msg(
    index: &StoreIndex,
    pending: &mut HashMap<String, PackageFilesIndex>,
    msg: WriteMsg,
) {
    match msg {
        WriteMsg::Replace { key, value } => {
            pending.insert(key, value);
        }
        WriteMsg::SideEffectsUpload { key, cache_key, current_files, response } => {
            let diff = load_pending_row(index, pending, &key)
                .and_then(|row| record_local_side_effects(row, &key, cache_key, &current_files));
            if let Some(response) = response {
                let _ = response.send(diff);
            }
        }
        WriteMsg::RemoteSideEffects { key, cache_key, diff } => {
            if let Some(row) = load_pending_row(index, pending, &key) {
                row.side_effects.get_or_insert_with(HashMap::new).insert(cache_key, diff);
            }
        }
        WriteMsg::QuarantineRemoteSideEffects { key, channel, envelope_digest } => {
            if let Some(row) = load_pending_row(index, pending, &key) {
                quarantine_digest(row, channel, envelope_digest);
            }
        }
    }
}

/// Diff the built package against the row's own files and record the result
/// under `cache_key`. A row written with another hash algorithm cannot be
/// diffed against.
fn record_local_side_effects(
    row: &mut PackageFilesIndex,
    key: &str,
    cache_key: String,
    current_files: &HashMap<String, CafsFileInfo>,
) -> Option<SideEffectsDiff> {
    if row.algo != crate::upload::HASH_ALGORITHM {
        tracing::warn!(
            target: "pacquet::store_index",
            key = %key,
            row_algo = %row.algo,
            "algo mismatch on base row; skip side-effects upload",
        );
        return None;
    }
    let diff = crate::upload::calculate_diff(&row.files, current_files);
    row.side_effects.get_or_insert_with(HashMap::new).insert(cache_key, diff.clone());
    Some(diff)
}

/// Remember a rejected remote envelope so it is not re-fetched, keeping only
/// the most recent [`MAX_QUARANTINED_REMOTE_SIDE_EFFECTS`] per channel.
fn quarantine_digest(row: &mut PackageFilesIndex, channel: String, envelope_digest: String) {
    let digests = row
        .remote_side_effects_quarantine
        .get_or_insert_with(HashMap::new)
        .entry(channel)
        .or_default();
    if !digests.contains(&envelope_digest) {
        digests.push(envelope_digest);
    }
    if digests.len() > MAX_QUARANTINED_REMOTE_SIDE_EFFECTS {
        digests.drain(..digests.len() - MAX_QUARANTINED_REMOTE_SIDE_EFFECTS);
    }
}

/// Return a mutable reference to the [`PackageFilesIndex`] row for
/// `key`, loading from `SQLite` when this is the row's first
/// sighting in the batch. Returns `None` (and logs at `debug!` /
/// `warn!` as appropriate) when no base row exists or the `SQLite`
/// read fails — both cases mean the caller should skip the
/// side-effects mutation without disturbing `pending`.
fn load_pending_row<'a>(
    index: &StoreIndex,
    pending: &'a mut HashMap<String, PackageFilesIndex>,
    key: &str,
) -> Option<&'a mut PackageFilesIndex> {
    use std::collections::hash_map::Entry;
    match pending.entry(key.to_string()) {
        Entry::Occupied(o) => Some(o.into_mut()),
        Entry::Vacant(v) => match index.get(key) {
            Ok(Some(r)) => Some(v.insert(r)),
            Ok(None) => {
                tracing::debug!(
                    target: "pacquet::store_index",
                    key = %key,
                    "no base row for side-effects upload; skip",
                );
                None
            }
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::store_index",
                    ?error,
                    key = %key,
                    "failed to read base row for side-effects upload",
                );
                None
            }
        },
    }
}

impl StoreIndexWriter {
    /// Queue one `(key, value)` to be flushed in the next transaction.
    ///
    /// Silently drops the entry if the writer task has exited (closed
    /// channel). Matches pnpm's graceful-degradation on failed writes:
    /// the install in flight still completes, the next install misses on
    /// this cache-key and re-downloads. The "channel closed" warning is
    /// logged only on the first failure per writer instance — every
    /// subsequent call would emit the same message, and on a 1352-
    /// snapshot install that's a thousand identical warnings drowning
    /// out real diagnostics.
    pub fn queue(&self, key: String, value: PackageFilesIndex) {
        self.send_msg(WriteMsg::Replace { key, value });
    }

    /// Queue a side-effects R/M/W: the writer task loads the row,
    /// computes [`crate::calculate_diff`] against the existing
    /// `files`, inserts `(cache_key → diff)` into the row's
    /// `side_effects` map, and re-flushes. Routing this through
    /// the writer's batch loop is what keeps multiple uploads for
    /// the same row commutative — see the doc-comment on the
    /// `WriteMsg::SideEffectsUpload` variant for the rationale.
    ///
    /// If no base row exists at `key`, the upload is silently
    /// skipped — there is nothing to layer the side-effects diff onto.
    pub fn queue_side_effects_upload(
        &self,
        key: String,
        cache_key: String,
        current_files: HashMap<String, CafsFileInfo>,
    ) {
        self.send_msg(WriteMsg::SideEffectsUpload {
            key,
            cache_key,
            current_files,
            response: None,
        });
    }

    pub fn queue_side_effects_upload_with_result(
        &self,
        key: String,
        cache_key: String,
        current_files: HashMap<String, CafsFileInfo>,
    ) -> Option<SideEffectsDiff> {
        let (response, result) = std::sync::mpsc::sync_channel(1);
        self.send_msg(WriteMsg::SideEffectsUpload {
            key,
            cache_key,
            current_files,
            response: Some(response),
        });
        result.recv().ok().flatten()
    }

    pub fn queue_remote_side_effects(&self, key: String, cache_key: String, diff: SideEffectsDiff) {
        self.send_msg(WriteMsg::RemoteSideEffects { key, cache_key, diff });
    }

    pub fn queue_remote_side_effects_quarantine(
        &self,
        key: String,
        channel: String,
        envelope_digest: String,
    ) {
        self.send_msg(WriteMsg::QuarantineRemoteSideEffects { key, channel, envelope_digest });
    }

    fn send_msg(&self, msg: WriteMsg) {
        if let Err(error) = self.tx.send(msg)
            && self.warn_on_send_failure.swap(false, Ordering::Relaxed)
        {
            tracing::warn!(
                target: "pacquet::store_index",
                ?error,
                "store-index writer channel closed; dropping queued row (further failures silenced)",
            );
        }
    }
}
