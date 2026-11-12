use super::{
    CachedFile, HashSet, Path,
    inventory::{CachedFileRemoval, check_unmodified_cached_files_sync, remove_cached_files_sync},
};
use pnpm_fs::OpenDir;
use pnpm_store_dir::{
    PackageFilesIndex, STORE_VERSION, StoreIndex, StoreIndexError, decode_package_files_index,
};
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
    sync::Arc,
};

const STORE_DIR_COMPONENTS: [&str; 2] = ["resolver-store", STORE_VERSION];
const STORE_INDEX_FAMILY: [&str; 4] =
    ["index.db", "index.db-wal", "index.db-shm", "index.db-journal"];
const FILES_INDEX_DIRS: [&str; 2] = ["index", "index-aliases"];

pub(super) struct CasBatchOutcome {
    pub(super) files_evicted: usize,
    pub(super) bytes_reclaimed: u64,
    pub(super) skipped: Vec<CachedFile>,
}

pub(super) fn store_index_family_bytes(cache_root: &OpenDir) -> io::Result<u64> {
    let Some(first) = cache_root.open_child_dir(OsStr::new(STORE_DIR_COMPONENTS[0]))? else {
        return Ok(0);
    };
    let Some(store_dir) = first.open_child_dir(OsStr::new(STORE_DIR_COMPONENTS[1]))? else {
        return Ok(0);
    };
    store_index_bytes_in(&store_dir)
}

pub(super) async fn evict_cas_entries(
    cache_root: &Arc<OpenDir>,
    cache_dir: &Path,
    pending: Vec<(CachedFile, String)>,
) -> CasBatchOutcome {
    let root = Arc::clone(cache_root);
    let root_path = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || evict_cas_entries_sync(&root, &root_path, pending))
        .await
        .unwrap_or(CasBatchOutcome { files_evicted: 0, bytes_reclaimed: 0, skipped: Vec::new() })
}

fn evict_cas_entries_sync(
    cache_root: &OpenDir,
    cache_dir: &Path,
    pending: Vec<(CachedFile, String)>,
) -> CasBatchOutcome {
    let (ready, mut skipped) = partition_unmodified_cas(cache_root, cache_dir, pending);
    if ready.is_empty() {
        return CasBatchOutcome { files_evicted: 0, bytes_reclaimed: 0, skipped };
    }
    let digests: HashSet<String> = ready
        .iter()
        .map(|(_, digest)| digest.clone())
        .collect();
    let index_reclaimed = match prune_store_index_sync(cache_root, &digests) {
        Ok(reclaimed) => reclaimed,
        Err(err) => {
            tracing::warn!(error = %err, "pruning resolver store index failed; retaining CAS files");
            skipped.extend(ready.into_iter().map(|(file, _)| file));
            return CasBatchOutcome { files_evicted: 0, bytes_reclaimed: 0, skipped };
        }
    };
    unlink_pruned_cas_files(cache_root, cache_dir, ready, index_reclaimed, skipped)
}

fn partition_unmodified_cas(
    cache_root: &OpenDir,
    cache_dir: &Path,
    pending: Vec<(CachedFile, String)>,
) -> (Vec<(CachedFile, String)>, Vec<CachedFile>) {
    let checks = cas_removal_targets(&pending);
    let unmodified = check_unmodified_cached_files_sync(cache_root, cache_dir, &checks);
    let mut ready = Vec::new();
    let mut skipped = Vec::new();
    for (item, ok) in pending.into_iter().zip(unmodified) {
        if ok {
            ready.push(item);
        } else {
            skipped.push(item.0);
        }
    }
    (ready, skipped)
}

fn unlink_pruned_cas_files(
    cache_root: &OpenDir,
    cache_dir: &Path,
    ready: Vec<(CachedFile, String)>,
    index_reclaimed: u64,
    mut skipped: Vec<CachedFile>,
) -> CasBatchOutcome {
    let targets = cas_removal_targets(&ready);
    let outcomes = remove_cached_files_sync(cache_root, cache_dir, &targets);
    let mut files_evicted = 0usize;
    let mut bytes_reclaimed = index_reclaimed;
    for ((file, _), removed) in ready.into_iter().zip(outcomes) {
        if removed {
            files_evicted += 1;
            bytes_reclaimed += file.size;
        } else {
            skipped.push(file);
        }
    }
    CasBatchOutcome { files_evicted, bytes_reclaimed, skipped }
}

fn cas_removal_targets(items: &[(CachedFile, String)]) -> Vec<CachedFileRemoval> {
    items
        .iter()
        .map(|(file, _)| CachedFileRemoval {
            path: file.path.clone(),
            max_effective_time: Some(file.effective_time),
        })
        .collect()
}

fn prune_store_index_sync(
    cache_root: &OpenDir,
    removed_digests: &HashSet<String>,
) -> io::Result<u64> {
    let Some(store_root) = open_store_dir(cache_root)? else {
        return Ok(0);
    };
    let before = store_index_bytes_in(&store_root)?;
    let Some(mut index) = StoreIndex::open_existing_at(&store_root).map_err(io::Error::other)?
    else {
        return Ok(0);
    };
    let mut keys = Vec::new();
    index
        .for_each_raw(|key, data| {
            if let Ok(pkg) = decode_package_files_index(&data)
                && package_references_removed_digest(&pkg, removed_digests)
            {
                keys.push(key);
            }
            Ok::<(), StoreIndexError>(())
        })
        .map_err(io::Error::other)?;
    index.delete_many(&keys).map_err(io::Error::other)?;
    drop(index);
    Ok(before.saturating_sub(store_index_bytes_in(&store_root)?))
}

fn open_store_dir(cache_root: &OpenDir) -> io::Result<Option<OpenDir>> {
    let first_name = STORE_DIR_COMPONENTS[0];
    if cache_root.has_symlink_child(OsStr::new(first_name))? {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("refusing symlinked cache directory component `{first_name}`"),
        ));
    }
    let Some(first) = cache_root.open_child_dir(OsStr::new(first_name))? else {
        return Ok(None);
    };
    let second_name = STORE_DIR_COMPONENTS[1];
    if first.has_symlink_child(OsStr::new(second_name))? {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("refusing symlinked cache directory component `{second_name}`"),
        ));
    }
    first.open_child_dir(OsStr::new(second_name))
}

fn store_index_bytes_in(store_dir: &OpenDir) -> io::Result<u64> {
    let mut total = 0u64;
    for name in STORE_INDEX_FAMILY {
        if let Some(stat) = store_dir.child_file_stat(OsStr::new(name))? {
            total = total.saturating_add(stat.len);
        }
    }
    for subdir_name in FILES_INDEX_DIRS {
        total = total.saturating_add(files_index_subdir_bytes(store_dir, subdir_name)?);
    }
    Ok(total)
}

fn files_index_subdir_bytes(store_dir: &OpenDir, subdir_name: &str) -> io::Result<u64> {
    if store_dir.has_symlink_child(OsStr::new(subdir_name))? {
        return Ok(0);
    }
    let Some(subdir) = store_dir.open_child_dir(OsStr::new(subdir_name))? else {
        return Ok(0);
    };
    let mut total = 0u64;
    for shard_name in subdir.entry_names()? {
        total = total.saturating_add(files_index_shard_bytes(&subdir, &shard_name)?);
    }
    Ok(total)
}

fn files_index_shard_bytes(subdir: &OpenDir, shard_name: &OsStr) -> io::Result<u64> {
    if subdir.has_symlink_child(shard_name)? {
        return Ok(0);
    }
    let Some(shard_dir) = subdir.open_child_dir(shard_name)? else {
        return Ok(0);
    };
    let mut total = 0u64;
    for entry_name in shard_dir.entry_names()? {
        if entry_name.as_encoded_bytes().starts_with(b".tmp") {
            continue;
        }
        if let Some(stat) = shard_dir.child_file_stat(&entry_name)? {
            total = total.saturating_add(stat.len);
        }
    }
    Ok(total)
}

fn package_references_removed_digest(
    pkg: &PackageFilesIndex,
    removed_digests: &HashSet<String>,
) -> bool {
    pkg.files
        .values()
        .any(|file| removed_digests.contains(&file.digest))
        || pkg.side_effects
            .iter()
            .flatten()
            .flat_map(|(_, diff)| diff.added.iter().flatten())
            .any(|(_, file)| removed_digests.contains(&file.digest))
}
