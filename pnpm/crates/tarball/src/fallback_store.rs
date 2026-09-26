//! Import from `fallbackStoreDir`, a read-only store consulted when a
//! package is missing from the store being installed into.
//!
//! A hit copies the row's files into the primary store, hashing each
//! one on the way, and queues the row on the primary store's index.
//! The package then lives in the primary store like any fetched one,
//! so linking never reaches into the read-only store.

use crate::{CachedCasPaths, PackageContentCheck, TarballError};
use dashmap::DashMap;
use pnpm_fs::file_mode::is_executable;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedReadonlyStoreIndex, StoreDir, StoreIndex,
    pkg_content_mismatch,
};
use std::{
    collections::HashMap,
    fs::File,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

/// The fallback store's index, opened once per process on first success.
/// Opened immutable: the fallback store is expected on a read-only
/// mount, where even a read-only WAL open fails creating its sidecars.
fn fallback_index(store_dir: &'static StoreDir) -> Option<SharedReadonlyStoreIndex> {
    static INDEXES: LazyLock<DashMap<PathBuf, SharedReadonlyStoreIndex>> =
        LazyLock::new(DashMap::new);
    if let Some(index) = INDEXES.get(store_dir.root()) {
        return Some(Arc::clone(&index));
    }
    let index = StoreIndex::shared_immutable_in(store_dir)?;
    Some(Arc::clone(
        INDEXES
            .entry(store_dir.root().to_path_buf())
            .or_insert(index)
            .value(),
    ))
}

/// The copied files and the row to record for them in the primary store.
pub(crate) struct ImportedFromFallback {
    pub(crate) cached: CachedCasPaths,
    pub(crate) row: PackageFilesIndex,
}

/// Copy the package stored under `cache_key` in `fallback_dir` into
/// `store_dir`. `Ok(None)` when the fallback store has no usable row or
/// any of its files is missing or does not match its recorded digest.
pub(crate) async fn import_from_fallback_store<Reporter: crate::Reporter>(
    fallback_dir: &'static StoreDir,
    store_dir: &'static StoreDir,
    cache_key: String,
    package_content_check: PackageContentCheck,
) -> Result<Option<ImportedFromFallback>, TarballError> {
    let Some(index) = fallback_index(fallback_dir) else { return Ok(None) };
    tokio::task::spawn_blocking(move || {
        let Some(row) = index
            .lock()
            .ok()
            .and_then(|guard| guard.get(&cache_key).ok().flatten())
        else {
            return Ok(None);
        };
        let mismatch = match package_content_check {
            PackageContentCheck::Strict | PackageContentCheck::Warn => {
                pkg_content_mismatch(row.manifest.as_ref(), &cache_key)
            }
            PackageContentCheck::Skip => None,
        };
        if let Some(mismatch) = mismatch {
            if package_content_check == PackageContentCheck::Strict {
                return Err(TarballError::UnexpectedPkgContentInStore { hint: mismatch.hint() });
            }
            Reporter::emit(&LogEvent::Global(GlobalLog {
                level: LogLevel::Warn,
                message: format!(
                    "Package name or version mismatch found while reading from the store. {}",
                    mismatch.hint(),
                ),
            }));
        }
        Ok(copy_row(fallback_dir, store_dir, row))
    })
    .await
    .map_err(TarballError::TaskJoin)?
}

fn copy_row(
    fallback_dir: &StoreDir,
    store_dir: &StoreDir,
    row: PackageFilesIndex,
) -> Option<ImportedFromFallback> {
    let mut files_map = HashMap::with_capacity(row.files.len());
    let mut files = HashMap::with_capacity(row.files.len());
    for (name, info) in row.files {
        let (path, info) = copy_file(fallback_dir, store_dir, &info)?;
        files_map.insert(name.clone(), path);
        files.insert(name, info);
    }
    let manifest = row.manifest;
    Some(ImportedFromFallback {
        cached: CachedCasPaths { files: files_map, manifest: manifest.clone() },
        // Side effects are left behind: their files were not copied.
        row: PackageFilesIndex {
            manifest,
            requires_build: row.requires_build,
            requires_prepare: row.requires_prepare,
            algo: row.algo,
            files,
            side_effects: None,
            remote_side_effects_quarantine: None,
        },
    })
}

fn copy_file(
    fallback_dir: &StoreDir,
    store_dir: &StoreDir,
    info: &CafsFileInfo,
) -> Option<(PathBuf, CafsFileInfo)> {
    let source = fallback_dir.cas_file_path_by_mode(&info.digest, info.mode)?;
    let mut reader = File::open(&source)
        .map_err(|error| {
            tracing::debug!(target: "pacquet::download", ?source, ?error, "fallback store file unreadable");
        })
        .ok()?;
    let (path, hash, size) = store_dir
        .write_cas_file_from_reader(&mut reader, is_executable(info.mode), Some(info.size))
        .map_err(|error| {
            tracing::debug!(target: "pacquet::download", ?source, ?error, "fallback store file not copied");
        })
        .ok()?;
    if format!("{hash:x}") != info.digest {
        tracing::debug!(target: "pacquet::download", ?source, "fallback store file does not match its digest");
        return None;
    }
    Some((path, crate::extract::cafs_file_info(&hash, info.mode, size)))
}
