pub(super) use self::cleanup::{CachedFileRemoval, prune_empty_cache_dirs, remove_cached_files};

mod cleanup;
use super::{CachedFile, HashSet, Path, PathBuf, Result, SystemTime, stat_effective_time};
use pnpm_fs::{ChildFileStat, OpenDir};
use pnpm_store_dir::STORE_VERSION;
use std::{
    ffi::{OsStr, OsString},
    io::{self, ErrorKind},
    sync::Arc,
    vec::IntoIter,
};

pub(super) struct CacheInventory {
    pub(super) root: Arc<OpenDir>,
    pub(super) files: Vec<CachedFile>,
    pub(super) non_evictable_bytes: u64,
}

enum DirPolicy {
    Evictable,
    CountOnly,
    Skip,
}

/// Walk `cache_dir` through root-anchored `O_NOFOLLOW` handles and classify
/// regular files into eviction candidates and non-evictable live overhead
/// (`layout.json`, `.cache-secret`, in-flight staging/replay/blob uploads, and
/// `shared-artifacts`). SQLite databases for the active `resolver-cache` and
/// `resolver-store` are accounted separately. Returns `Ok(None)` when
/// `cache_dir` does not exist.
pub(super) async fn inventory_cache_dir(
    cache_dir: &Path,
    excluded_dirs: &HashSet<PathBuf>,
) -> Result<Option<CacheInventory>> {
    let root = cache_dir.to_path_buf();
    let excluded = excluded_dirs.clone();
    tokio::task::spawn_blocking(move || inventory_cache_dir_sync(&root, &excluded))
        .await
        .map_err(|err| io::Error::other(err.to_string()))?
}

fn inventory_cache_dir_sync(
    cache_dir: &Path,
    excluded_dirs: &HashSet<PathBuf>,
) -> Result<Option<CacheInventory>> {
    let root_dir = match OpenDir::open(cache_dir) {
        Ok(dir) => Arc::new(dir),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let entries = root_dir.entry_names()?;
    let mut inventory =
        CacheInventory { root: Arc::clone(&root_dir), files: Vec::new(), non_evictable_bytes: 0 };
    for name in entries {
        if excluded_dirs.contains(&cache_dir.join(&name)) {
            continue;
        }
        inspect_root_entry(cache_dir, &root_dir, &name, excluded_dirs, &mut inventory);
    }
    Ok(Some(inventory))
}

fn inspect_root_entry(
    cache_dir: &Path,
    root_dir: &OpenDir,
    name: &OsStr,
    excluded_dirs: &HashSet<PathBuf>,
    inventory: &mut CacheInventory,
) {
    let entry_path = cache_dir.join(name);
    match root_dir.open_child_dir(name) {
        Ok(Some(child_dir)) => {
            match root_dir_policy(name) {
                DirPolicy::Skip => {}
                DirPolicy::CountOnly => {
                    inventory.non_evictable_bytes += count_dir_bytes_lenient(child_dir);
                }
                DirPolicy::Evictable => {
                    collect_evictable_tree(
                        cache_dir,
                        child_dir,
                        excluded_dirs,
                        &mut inventory.files,
                    );
                }
            }
            return;
        }
        Ok(None) => {}
        Err(err) => {
            log_skipped_entry(&entry_path, &err);
            return;
        }
    }
    match root_dir.child_file_stat(name) {
        Ok(Some(stat)) => inspect_root_file(cache_dir, name, stat, inventory),
        Ok(None) => {}
        Err(err) => log_skipped_entry(&entry_path, &err),
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

fn inspect_root_file(
    cache_dir: &Path,
    name: &OsStr,
    stat: ChildFileStat,
    inventory: &mut CacheInventory,
) {
    if is_root_marker(name) {
        inventory.non_evictable_bytes += stat.len;
        return;
    }
    let (effective_time, age) = stat_effective_time(stat);
    inventory.files.push(CachedFile {
        path: cache_dir.join(name),
        size: stat.len,
        effective_time,
        age,
    });
}

fn is_root_marker(name: &OsStr) -> bool {
    name == pnpr_storage::layout::LAYOUT_FILE || name == pnpr_config::CACHE_SECRET_MARKER
}

fn collect_evictable_tree(
    cache_dir: &Path,
    root: OpenDir,
    excluded_dirs: &HashSet<PathBuf>,
    files: &mut Vec<CachedFile>,
) {
    let Some(entries) = list_dir_entries_logged(&root) else {
        return;
    };
    let mut stack = vec![(root, entries)];
    while let Some((dir, iter)) = stack.last_mut() {
        let Some(name) = iter.next() else {
            stack.pop();
            continue;
        };
        if let Some(child_frame) = inspect_tree_entry(cache_dir, dir, &name, excluded_dirs, files) {
            stack.push(child_frame);
        }
    }
}

fn inspect_tree_entry(
    cache_dir: &Path,
    dir: &OpenDir,
    name: &OsStr,
    excluded_dirs: &HashSet<PathBuf>,
    files: &mut Vec<CachedFile>,
) -> Option<(OpenDir, IntoIter<OsString>)> {
    let path = dir.path().join(name);
    if excluded_dirs.contains(&path) {
        return None;
    }
    match dir.open_child_dir(name) {
        Ok(Some(subdir)) => {
            let entries = list_dir_entries_logged(&subdir)?;
            return Some((subdir, entries));
        }
        Ok(None) => {}
        Err(err) => {
            log_skipped_entry(&path, &err);
            return None;
        }
    }
    if is_managed_sqlite_file(cache_dir, &path) {
        return None;
    }
    match dir.child_file_stat(name) {
        Ok(Some(stat)) => {
            let (effective_time, age) = stat_effective_time(stat);
            files.push(CachedFile { path, size: stat.len, effective_time, age });
        }
        Ok(None) => {}
        Err(err) => log_skipped_entry(&path, &err),
    }
    None
}

fn count_dir_bytes_lenient(root: OpenDir) -> u64 {
    let Some(entries) = list_dir_entries_logged(&root) else {
        return 0;
    };
    let mut bytes = 0u64;
    let mut stack = vec![(root, entries)];
    while let Some((dir, iter)) = stack.last_mut() {
        let Some(name) = iter.next() else {
            stack.pop();
            continue;
        };
        if let Some(frame) = count_child_entry_lenient(dir, &name, &mut bytes) {
            stack.push(frame);
        }
    }
    bytes
}

fn count_child_entry_lenient(
    dir: &OpenDir,
    name: &OsStr,
    bytes: &mut u64,
) -> Option<(OpenDir, IntoIter<OsString>)> {
    let path = dir.path().join(name);
    match dir.open_child_dir(name) {
        Ok(Some(subdir)) => {
            return list_dir_entries_logged(&subdir).map(|entries| (subdir, entries));
        }
        Ok(None) => {}
        Err(err) => {
            log_skipped_entry(&path, &err);
            return None;
        }
    }
    match dir.child_file_stat(name) {
        Ok(Some(stat)) => *bytes += stat.len,
        Ok(None) => {}
        Err(err) => log_skipped_entry(&path, &err),
    }
    None
}

fn list_dir_entries_logged(dir: &OpenDir) -> Option<IntoIter<OsString>> {
    match dir.entry_names() {
        Ok(names) => Some(names.into_iter()),
        Err(err) => {
            log_skipped_entry(dir.path(), &err);
            None
        }
    }
}

fn log_skipped_entry(path: &Path, err: &io::Error) {
    tracing::warn!(
        path = %path.display(),
        error = %err,
        "skipping unreadable cache entry during inventory",
    );
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
        ["resolver-store", STORE_VERSION, file] => is_sqlite_family(file, "index.db"),
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
        ["resolver-store", STORE_VERSION, "tmp", ..] => true,
        ["resolver-store", STORE_VERSION, "files", file] => file.starts_with("stream"),
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
    let ["resolver-store", STORE_VERSION, "files", shard, file] = parts.as_slice() else {
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
