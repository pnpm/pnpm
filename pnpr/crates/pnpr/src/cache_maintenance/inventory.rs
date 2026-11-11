use super::{CachedFile, Path, PathBuf, Result, file_effective_time, fs};
use std::{cmp::Reverse, ffi::OsStr};

pub(super) struct CacheInventory {
    pub(super) files: Vec<CachedFile>,
    pub(super) non_evictable_bytes: u64,
}

enum DirPolicy {
    Evictable,
    CountOnly,
    Skip,
}

/// Walk `cache_dir` and classify regular files into eviction candidates and
/// non-evictable live overhead (`layout.json`, `.cache-secret`, in-flight
/// staging/replay/blob uploads, and `shared-artifacts`). SQLite databases
/// under `resolver-cache` and `resolver-store` are accounted separately.
pub(super) async fn inventory_cache_dir(cache_dir: &Path) -> Result<CacheInventory> {
    let mut inventory = CacheInventory { files: Vec::new(), non_evictable_bytes: 0 };
    let Ok(mut root_entries) = fs::read_dir(cache_dir).await else {
        return Ok(inventory);
    };
    while let Ok(Some(entry)) = root_entries.next_entry().await {
        inspect_root_entry(cache_dir, &entry, &mut inventory).await;
    }
    Ok(inventory)
}

async fn inspect_root_entry(
    cache_dir: &Path,
    entry: &tokio::fs::DirEntry,
    inventory: &mut CacheInventory,
) {
    let Ok(ft) = entry.file_type().await else {
        return;
    };
    let name = entry.file_name();
    if ft.is_dir() {
        match root_dir_policy(&name) {
            DirPolicy::Skip => {}
            DirPolicy::CountOnly => {
                inventory.non_evictable_bytes += sum_dir_bytes(entry.path()).await;
            }
            DirPolicy::Evictable => {
                collect_evictable_tree(cache_dir, entry.path(), &mut inventory.files).await;
            }
        }
    } else if ft.is_file() {
        inspect_root_file(&name, entry, inventory).await;
    }
}

fn root_dir_policy(name: &OsStr) -> DirPolicy {
    let Some(raw) = name.to_str() else {
        return DirPolicy::Evictable;
    };
    match raw {
        "lost+found" => DirPolicy::Skip,
        ".staging" | ".publish-replay" | ".uploads" | "shared-artifacts" => DirPolicy::CountOnly,
        _ => DirPolicy::Evictable,
    }
}

async fn inspect_root_file(
    name: &OsStr,
    entry: &tokio::fs::DirEntry,
    inventory: &mut CacheInventory,
) {
    let Ok(metadata) = entry.metadata().await else {
        return;
    };
    if is_root_marker(name) {
        inventory.non_evictable_bytes += metadata.len();
        return;
    }
    let (effective_time, age) = file_effective_time(&metadata);
    inventory.files.push(CachedFile {
        path: entry.path(),
        size: metadata.len(),
        effective_time,
        age,
    });
}

fn is_root_marker(name: &OsStr) -> bool {
    name == pnpr_storage::layout::LAYOUT_FILE || name == pnpr_config::CACHE_SECRET_MARKER
}

async fn sum_dir_bytes(root: PathBuf) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else {
                continue;
            };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file()
                && let Ok(meta) = entry.metadata().await
            {
                total += meta.len();
            }
        }
    }
    total
}

async fn collect_evictable_tree(cache_dir: &Path, root: PathBuf, files: &mut Vec<CachedFile>) {
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            inspect_tree_entry(cache_dir, &entry, &mut stack, files).await;
        }
    }
}

async fn inspect_tree_entry(
    cache_dir: &Path,
    entry: &tokio::fs::DirEntry,
    stack: &mut Vec<PathBuf>,
    files: &mut Vec<CachedFile>,
) {
    let Ok(ft) = entry.file_type().await else {
        return;
    };
    let path = entry.path();
    if ft.is_dir() {
        stack.push(path);
    } else if ft.is_file()
        && !is_managed_sqlite_file(cache_dir, &path)
        && let Ok(metadata) = entry.metadata().await
    {
        let (effective_time, age) = file_effective_time(&metadata);
        files.push(CachedFile { path, size: metadata.len(), effective_time, age });
    }
}

pub(super) fn is_managed_sqlite_file(cache_dir: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(cache_dir) else {
        return false;
    };
    let parts: Vec<&str> = rel
        .iter()
        .filter_map(OsStr::to_str)
        .collect();
    match parts.as_slice() {
        ["resolver-cache", file] => is_sqlite_family(file, "lockfile-verdicts.sqlite"),
        ["resolver-store", _, file] => is_sqlite_family(file, "index.db"),
        _ => false,
    }
}

fn is_sqlite_family(file: &str, base: &str) -> bool {
    file == base
        || file
            .strip_prefix(base)
            .is_some_and(|suffix| matches!(suffix, "-wal" | "-shm" | "-journal"))
}

pub(super) fn is_cache_tmp_file(cache_dir: &Path, path: &Path) -> bool {
    if pnpr_storage::is_atomic_write_temp_path(path) {
        return true;
    }
    if path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with(".tmp") || is_mirror_temp_name(name))
    {
        return true;
    }
    let Ok(rel) = path.strip_prefix(cache_dir) else {
        return false;
    };
    let parts: Vec<&str> = rel
        .iter()
        .filter_map(OsStr::to_str)
        .collect();
    match parts.as_slice() {
        ["resolver-store", _, "tmp", ..] => true,
        ["resolver-store", _, "files", file] => file.starts_with("stream"),
        _ => false,
    }
}

fn is_mirror_temp_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tmp") else {
        return false;
    };
    let mut parts = stem.rsplitn(3, '.');
    let (Some(counter), Some(pid), Some(base)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !base.is_empty()
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !counter.is_empty()
        && counter.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn cas_digest_of_store_file(cache_dir: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(cache_dir).ok()?;
    let parts: Vec<&str> = rel
        .iter()
        .filter_map(OsStr::to_str)
        .collect();
    let ["resolver-store", _, "files", shard, file] = parts.as_slice() else {
        return None;
    };
    if shard.len() != 2
        || !shard.bytes().all(|byte| byte.is_ascii_hexdigit())
        || file.starts_with('.')
    {
        return None;
    }
    Some(format!("{shard}{}", file.strip_suffix("-exec").unwrap_or(file)))
}

pub(super) async fn prune_empty_cache_dirs(cache_dir: &Path) -> Result<()> {
    let Ok(mut root_entries) = fs::read_dir(cache_dir).await else {
        return Ok(());
    };
    let mut dirs = Vec::new();
    while let Ok(Some(entry)) = root_entries.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if ft.is_dir() && matches!(root_dir_policy(&entry.file_name()), DirPolicy::Evictable) {
            collect_prunable_subdirs(cache_dir, entry.path(), &mut dirs).await;
        }
    }
    dirs.sort_by_key(|dir| Reverse(dir.as_os_str().len()));
    for dir in dirs {
        let _ = fs::remove_dir(dir).await;
    }
    Ok(())
}

async fn collect_prunable_subdirs(cache_dir: &Path, root: PathBuf, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        if !is_protected_cache_dir(cache_dir, &dir) {
            out.push(dir.clone());
        }
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.is_ok_and(|ft| ft.is_dir()) {
                stack.push(entry.path());
            }
        }
    }
}

fn is_protected_cache_dir(cache_dir: &Path, dir: &Path) -> bool {
    let Ok(rel) = dir.strip_prefix(cache_dir) else {
        return true;
    };
    let parts: Vec<&str> = rel
        .iter()
        .filter_map(OsStr::to_str)
        .collect();
    matches!(
        parts.as_slice(),
        ["resolver-cache" | "resolver-store"]
            | ["resolver-store", _]
            | ["resolver-store", _, "files"]
            | ["resolver-store", _, "files", _],
    )
}
