//! WRITE-path orchestrator: re-CAFS a post-build package directory,
//! diff it against the pristine `PackageFilesIndex.files` row, and
//! seed the side-effects cache by re-queueing the mutated row through
//! [`StoreIndexWriter`].

use crate::{
    AddFilesFromDirError, BundledPackageDirs, CafsFileInfo, SideEffectsDiff, StoreDir,
    StoreIndexError, StoreIndexWriter, add_files_from_dir_with_bundled,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

/// Error type of [`upload()`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum UploadError {
    #[diagnostic(transparent)]
    AddFilesFromDir(#[error(source)] AddFilesFromDirError),
    #[diagnostic(transparent)]
    StoreIndex(#[error(source)] StoreIndexError),
}

/// Digest algorithm pacquet writes into `PackageFilesIndex.algo`.
/// Held as a constant so the read-modify-write path can check the
/// existing row's algorithm before appending a side-effects diff.
pub const HASH_ALGORITHM: &str = "sha512";

/// Re-hash the built package directory and queue a side-effects
/// R/M/W against the row at `files_index_file`.
///
/// The actual load-existing → apply-diff → write-back happens
/// inside the writer task ([`StoreIndexWriter::queue_side_effects_upload`])
/// so concurrent uploads for the same row stay commutative.
///
/// Behaviour at the writer side:
///
/// - Build output contains symlinks that cannot be recorded (see
///   [`crate::add_files_from_dir()`]) → remove the matching cache entry and
///   wait for persistence; read, write, or unavailable-writer errors propagate.
/// - No base row at `files_index_file` → silent skip.
/// - Existing row's `algo` differs from [`HASH_ALGORITHM`] → log
///   at `warn!` and skip (an algorithm mismatch is demoted to a
///   warning here because at this point in the install we've
///   already done the build and the cache write is best-effort).
/// - Otherwise the row's `side_effects[side_effects_cache_key] =
///   diff` is set and the mutated row gets re-queued for the
///   batch flush.
pub fn upload(
    store_dir: &StoreDir,
    built_pkg_location: &Path,
    files_index_file: &str,
    side_effects_cache_key: &str,
    writer: &StoreIndexWriter,
) -> Result<(), UploadError> {
    let Some(bundled) = bundled_package_dirs(built_pkg_location, files_index_file, writer) else {
        return Ok(());
    };
    let added = add_files_from_dir_with_bundled(store_dir, built_pkg_location, bundled)
        .map_err(UploadError::AddFilesFromDir)?;
    if added.has_unrecorded_symlinks {
        writer
            .queue_side_effects_invalidation(
                files_index_file.to_string(),
                side_effects_cache_key.to_string(),
            )
            .map_err(UploadError::StoreIndex)?;
        return Ok(());
    }
    writer.queue_side_effects_upload(
        files_index_file.to_string(),
        side_effects_cache_key.to_string(),
        added.files,
    );
    Ok(())
}

pub fn upload_with_diff(
    store_dir: &StoreDir,
    built_pkg_location: &Path,
    files_index_file: &str,
    side_effects_cache_key: &str,
    writer: &StoreIndexWriter,
) -> Result<Option<SideEffectsDiff>, UploadError> {
    let Some(bundled) = bundled_package_dirs(built_pkg_location, files_index_file, writer) else {
        return Ok(None);
    };
    let added = add_files_from_dir_with_bundled(store_dir, built_pkg_location, bundled)
        .map_err(UploadError::AddFilesFromDir)?;
    if added.has_unrecorded_symlinks {
        writer
            .queue_side_effects_invalidation(
                files_index_file.to_string(),
                side_effects_cache_key.to_string(),
            )
            .map_err(UploadError::StoreIndex)?;
        return Ok(None);
    }
    Ok(writer.queue_side_effects_upload_with_result(
        files_index_file.to_string(),
        side_effects_cache_key.to_string(),
        added.files,
    ))
}

/// The bundled dependencies the pristine row at `files_index_file` has
/// files in. Read through `writer` so a row queued by the fetch that
/// precedes the build is visible.
///
/// `None` when the built package has a `node_modules` directory but the
/// row cannot be read. A diff without its bundled files would record them
/// as deleted, so the caller skips the upload.
fn bundled_package_dirs(
    built_pkg_location: &Path,
    files_index_file: &str,
    writer: &StoreIndexWriter,
) -> Option<Option<BundledPackageDirs>> {
    if !built_pkg_location.join("node_modules").is_dir() {
        return Some(None);
    }
    let paths = writer.read_file_paths(files_index_file.to_string())?;
    Some(BundledPackageDirs::from_files(paths.iter().map(String::as_str)))
}

/// Set-difference over file digests, and over modes where the host records them.
///
/// `base`     — the pristine `PackageFilesIndex.files` map (pre-build).
/// `current`  — the rehashed map produced by [`crate::add_files_from_dir()`].
///
/// Off Unix [`crate::add_files_from_dir()`] reports a fixed mode, so a mode
/// difference from `base` is not a change the build made. Those
/// differences are ignored. A digest change is still recorded.
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
    let all_files: BTreeSet<&str> = base
        .keys()
        .chain(current.keys())
        .map(String::as_str)
        .collect();
    for file in all_files {
        match (base.get(file), current.get(file)) {
            (Some(_), None) => deleted.push(file.to_string()),
            (None, Some(now)) => {
                added.insert(file.to_string(), clone_info(now));
            }
            (Some(before), Some(now))
                if before.digest != now.digest
                    || (cfg!(any(unix, target_os = "wasi")) && before.mode != now.mode) =>
            {
                added.insert(file.to_string(), clone_info(now));
            }
            _ => {}
        }
    }
    SideEffectsDiff {
        added: (!added.is_empty()).then_some(added),
        deleted: (!deleted.is_empty()).then_some(deleted),
        remote_origin: None,
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
