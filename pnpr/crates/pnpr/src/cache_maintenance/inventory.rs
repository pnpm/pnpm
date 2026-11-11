use super::{CachedFile, HashSet, Path, PathBuf, Result, SystemTime, stat_effective_time};
use pnpm_fs::{ChildFileStat, OpenDir};
use pnpm_store_dir::STORE_VERSION;
use std::{
    cmp::Reverse,
    ffi::{OsStr, OsString},
    io::{self, ErrorKind},
};

pub(super) struct CacheInventory {
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
/// `resolver-store` are accounted separately.
pub(super) async fn inventory_cache_dir(
    cache_dir: &Path,
    excluded_dirs: &HashSet<PathBuf>,
) -> Result<CacheInventory> {
    let root = cache_dir.to_path_buf();
    let excluded = excluded_dirs.clone();
    tokio::task::spawn_blocking(move || inventory_cache_dir_sync(&root, &excluded))
        .await
        .map_err(|err| io::Error::other(err.to_string()))?
}

fn inventory_cache_dir_sync(
    cache_dir: &Path,
    excluded_dirs: &HashSet<PathBuf>,
) -> Result<CacheInventory> {
    let mut inventory = CacheInventory { files: Vec::new(), non_evictable_bytes: 0 };
    let root_dir = match OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(inventory),
        Err(err) => return Err(err.into()),
    };
    for name in root_dir.entry_names()? {
        if excluded_dirs.contains(&cache_dir.join(&name)) {
            continue;
        }
        inspect_root_entry(cache_dir, &root_dir, &name, excluded_dirs, &mut inventory)?;
    }
    Ok(inventory)
}

fn inspect_root_entry(
    cache_dir: &Path,
    root_dir: &OpenDir,
    name: &OsStr,
    excluded_dirs: &HashSet<PathBuf>,
    inventory: &mut CacheInventory,
) -> io::Result<()> {
    if let Some(child_dir) = root_dir.open_child_dir(name)? {
        return match root_dir_policy(name) {
            DirPolicy::Skip => Ok(()),
            DirPolicy::CountOnly => {
                inventory.non_evictable_bytes += sum_dir_bytes(child_dir)?;
                Ok(())
            }
            DirPolicy::Evictable => {
                collect_evictable_tree(cache_dir, child_dir, excluded_dirs, &mut inventory.files)
            }
        };
    }
    if let Some(stat) = root_dir.child_file_stat(name)? {
        inspect_root_file(cache_dir, name, stat, inventory);
    }
    Ok(())
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

fn sum_dir_bytes(root: OpenDir) -> io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for name in dir.entry_names()? {
            if let Some(subdir) = dir.open_child_dir(&name)? {
                stack.push(subdir);
            } else if let Some(stat) = dir.child_file_stat(&name)? {
                total += stat.len;
            }
        }
    }
    Ok(total)
}

fn collect_evictable_tree(
    cache_dir: &Path,
    root: OpenDir,
    excluded_dirs: &HashSet<PathBuf>,
    files: &mut Vec<CachedFile>,
) -> io::Result<()> {
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for name in dir.entry_names()? {
            inspect_tree_entry(cache_dir, &dir, &name, excluded_dirs, &mut stack, files)?;
        }
    }
    Ok(())
}

fn inspect_tree_entry(
    cache_dir: &Path,
    dir: &OpenDir,
    name: &OsStr,
    excluded_dirs: &HashSet<PathBuf>,
    stack: &mut Vec<OpenDir>,
    files: &mut Vec<CachedFile>,
) -> io::Result<()> {
    let path = dir.path().join(name);
    if excluded_dirs.contains(&path) {
        return Ok(());
    }
    if let Some(subdir) = dir.open_child_dir(name)? {
        stack.push(subdir);
        return Ok(());
    }
    if !is_managed_sqlite_file(cache_dir, &path)
        && let Some(stat) = dir.child_file_stat(name)?
    {
        let (effective_time, age) = stat_effective_time(stat);
        files.push(CachedFile { path, size: stat.len, effective_time, age });
    }
    Ok(())
}

pub(super) async fn remove_cached_file(
    cache_dir: &Path,
    path: &Path,
    max_effective_time: Option<SystemTime>,
) -> bool {
    let root = cache_dir.to_path_buf();
    let target = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        remove_cached_file_sync(&root, &target, max_effective_time).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

fn remove_cached_file_sync(
    cache_dir: &Path,
    path: &Path,
    max_effective_time: Option<SystemTime>,
) -> io::Result<bool> {
    let Ok(rel) = path.strip_prefix(cache_dir) else {
        return Ok(false);
    };
    let (Some(parent_rel), Some(leaf)) = (rel.parent(), rel.file_name()) else {
        return Ok(false);
    };
    let root = match OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let Some(parent) = root.open_descendant_dir(parent_rel)? else {
        return Ok(false);
    };
    if let Some(limit_time) = max_effective_time {
        let Some(stat) = parent.child_file_stat(leaf)? else {
            return Ok(false);
        };
        if stat.modified.max(stat.accessed) > limit_time {
            return Ok(false);
        }
    }
    parent.remove_child_file(leaf)
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

pub(super) async fn prune_empty_cache_dirs(cache_dir: &Path) -> Result<()> {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || prune_empty_cache_dirs_sync(&root))
        .await
        .map_err(|err| io::Error::other(err.to_string()))?
}

fn prune_empty_cache_dirs_sync(cache_dir: &Path) -> Result<()> {
    let root = match OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let mut dirs: Vec<(PathBuf, OsString)> = Vec::new();
    for name in root.entry_names()? {
        if matches!(root_dir_policy(&name), DirPolicy::Evictable)
            && let Some(child) = root.open_child_dir(&name)?
        {
            collect_prunable_subdirs(cache_dir, child, Path::new(""), name, &mut dirs)?;
        }
    }
    dirs.sort_by_key(|(parent_rel, leaf)| Reverse((parent_rel.as_os_str().len(), leaf.len())));
    for (parent_rel, leaf) in dirs {
        if let Ok(Some(parent)) = root.open_descendant_dir(&parent_rel) {
            let _ = parent.remove_empty_child_dir(&leaf);
        }
    }
    Ok(())
}

fn collect_prunable_subdirs(
    cache_dir: &Path,
    root: OpenDir,
    root_parent_rel: &Path,
    root_leaf: OsString,
    out: &mut Vec<(PathBuf, OsString)>,
) -> io::Result<()> {
    let mut stack = vec![(root, root_parent_rel.to_path_buf(), root_leaf)];
    while let Some((dir, parent_rel, leaf)) = stack.pop() {
        let rel = parent_rel.join(&leaf);
        if !is_protected_cache_dir(cache_dir, &cache_dir.join(&rel)) {
            out.push((parent_rel, leaf));
        }
        for child_name in dir.entry_names()? {
            if let Some(subdir) = dir.open_child_dir(&child_name)? {
                stack.push((subdir, rel.clone(), child_name));
            }
        }
    }
    Ok(())
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
            | ["resolver-store", STORE_VERSION]
            | ["resolver-store", STORE_VERSION, "files"]
            | ["resolver-store", STORE_VERSION, "files", _],
    )
}
