//! WRITE-path orchestrator: re-CAFS a post-build package directory,
//! diff it against the pristine `PackageFilesIndex.files` row, and
//! seed the side-effects cache by re-queueing the mutated row through
//! [`StoreIndexWriter`].
//!
//! Ports pnpm's
//! [`storeController.upload`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/store/controller/src/storeController/index.ts#L90-L99)
//! and the worker-side body at
//! [`worker/src/start.ts:312-383`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/worker/src/start.ts#L312-L383).

use crate::{
    AddFilesFromDirError, CafsFileInfo, SideEffectsDiff, StoreDir, StoreIndexWriter,
    add_files_from_dir,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::Arc,
};

/// Error type of [`upload()`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UploadError {
    #[diagnostic(transparent)]
    AddFilesFromDir(#[error(source)] AddFilesFromDirError),
}

/// Digest algorithm pacquet writes into `PackageFilesIndex.algo`.
/// Held as a constant so the read-modify-write path can check the
/// existing row's algorithm before appending a side-effects diff,
/// matching upstream's [`ALGO_MISMATCH`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/worker/src/start.ts#L358-L364)
/// guard.
pub const HASH_ALGORITHM: &str = "sha512";

/// Re-hash the built package directory and queue a side-effects
/// R/M/W against the row at `files_index_file`.
///
/// The actual load-existing → apply-diff → write-back happens
/// inside the writer task ([`StoreIndexWriter::queue_side_effects_upload`])
/// so concurrent uploads for the same row stay commutative — a
/// second upload to the same row builds on the first's mutation
/// rather than racing against a stale read.
///
/// Behaviour at the writer side mirrors `pnpm/pnpm@7e3145f9fc:worker/src/start.ts:342-371`:
///
/// - No base row at `files_index_file` → silent skip (upstream's
///   `if (!existingFilesIndex) return`).
/// - Existing row's `algo` differs from [`HASH_ALGORITHM`] → log
///   at `warn!` and skip (upstream's `ALGO_MISMATCH` error,
///   demoted to a warning here because at this point in the
///   install we've already done the build and the cache write is
///   best-effort).
/// - Otherwise the row's `side_effects[side_effects_cache_key] =
///   diff` is set and the mutated row gets re-queued for the
///   batch flush.
pub fn upload(
    store_dir: &StoreDir,
    built_pkg_location: &Path,
    files_index_file: &str,
    side_effects_cache_key: &str,
    writer: &Arc<StoreIndexWriter>,
) -> Result<(), UploadError> {
    let added =
        add_files_from_dir(store_dir, built_pkg_location).map_err(UploadError::AddFilesFromDir)?;
    writer.queue_side_effects_upload(
        files_index_file.to_string(),
        side_effects_cache_key.to_string(),
        added.files,
    );
    Ok(())
}

/// Set-difference over file digests + modes.  Mirrors
/// `pnpm/pnpm@7e3145f9fc:worker/src/start.ts:411-434`.
///
/// `base`     — the pristine `PackageFilesIndex.files` map (pre-build).
/// `current`  — the rehashed map produced by [`add_files_from_dir()`].
///
/// Returns a [`SideEffectsDiff`] whose `added` entry covers files
/// present in `current` that either don't appear in `base` or whose
/// `digest`/`mode` differ from the base, and whose `deleted` entry
/// lists files present in `base` but absent in `current`. Both
/// fields use `Option<…>` with `skip_serializing_if = is_none`
/// (see `SideEffectsDiff`), so an empty side of the diff
/// round-trips through msgpack the same way pnpm's does.
pub fn calculate_diff(
    base: &HashMap<String, CafsFileInfo>,
    current: &HashMap<String, CafsFileInfo>,
) -> SideEffectsDiff {
    let mut added: HashMap<String, CafsFileInfo> = HashMap::new();
    let mut deleted: Vec<String> = Vec::new();
    // `BTreeSet` so iteration order is deterministic. The returned
    // `deleted` vector ends up sorted lexicographically; byte-
    // stability of the eventual msgpack payload is provided
    // separately by `SideEffectsDiff.added`'s sorted-map
    // serializer (see `serialize_sorted_map_opt` in `store_index.rs`),
    // since `HashMap` iteration on its own remains unordered.
    let all_files: BTreeSet<&str> = base.keys().chain(current.keys()).map(String::as_str).collect();
    for file in all_files {
        match (base.get(file), current.get(file)) {
            (Some(_), None) => deleted.push(file.to_string()),
            (None, Some(now)) => {
                added.insert(file.to_string(), clone_info(now));
            }
            (Some(before), Some(now)) if before.digest != now.digest || before.mode != now.mode => {
                added.insert(file.to_string(), clone_info(now));
            }
            _ => {}
        }
    }
    SideEffectsDiff {
        added: (!added.is_empty()).then_some(added),
        deleted: (!deleted.is_empty()).then_some(deleted),
    }
}

fn clone_info(info: &CafsFileInfo) -> CafsFileInfo {
    CafsFileInfo {
        digest: info.digest.clone(),
        mode: info.mode,
        size: info.size,
        checked_at: info.checked_at,
    }
}

#[cfg(test)]
mod tests;
