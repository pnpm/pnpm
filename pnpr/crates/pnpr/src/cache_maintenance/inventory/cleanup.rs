use super::{
    DirPolicy, OpenDir, Path, PathBuf, Result, STORE_VERSION, SystemTime, root_dir_policy,
};
use std::{
    cmp::Reverse,
    ffi::{OsStr, OsString},
    io::{self, ErrorKind},
    vec::IntoIter,
};

pub(in crate::cache_maintenance) struct CachedFileRemoval {
    pub(in crate::cache_maintenance) path: PathBuf,
    pub(in crate::cache_maintenance) max_effective_time: Option<SystemTime>,
}

struct GroupedRemoval<'a> {
    parent_rel: &'a Path,
    leaf: &'a OsStr,
    index: usize,
    limit_time: Option<SystemTime>,
}

pub(in crate::cache_maintenance) async fn remove_cached_files(
    cache_dir: &Path,
    targets: Vec<CachedFileRemoval>,
) -> Vec<bool> {
    if targets.is_empty() {
        return Vec::new();
    }
    let count = targets.len();
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || remove_cached_files_sync(&root, &targets))
        .await
        .unwrap_or_else(|_| vec![false; count])
}

fn remove_cached_files_sync(cache_dir: &Path, targets: &[CachedFileRemoval]) -> Vec<bool> {
    let mut results = vec![false; targets.len()];
    let root = match OpenDir::open(cache_dir) {
        Ok(root) => root,
        Err(err) => {
            tracing::warn!(error = %err, "cache file removal failed to open cache root");
            return results;
        }
    };
    let mut grouped = collect_parent_grouped_removals(cache_dir, targets);
    grouped.sort_by(|left, right| left.parent_rel.cmp(right.parent_rel));
    let mut current_parent: Option<(&Path, Option<OpenDir>)> = None;
    for item in grouped {
        if current_parent
            .as_ref()
            .is_none_or(|(cached, _)| *cached != item.parent_rel)
        {
            drop(current_parent.take());
            let opened = open_parent_dir_logged(&root, item.parent_rel);
            current_parent = Some((item.parent_rel, opened));
        }
        if let Some((_, Some(parent))) = &current_parent {
            results[item.index] = unlink_if_unmodified(parent, item.leaf, item.limit_time);
        }
    }
    results
}

fn collect_parent_grouped_removals<'a>(
    cache_dir: &Path,
    targets: &'a [CachedFileRemoval],
) -> Vec<GroupedRemoval<'a>> {
    targets
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let rel = item.path.strip_prefix(cache_dir).ok()?;
            Some(GroupedRemoval {
                parent_rel: rel.parent()?,
                leaf: rel.file_name()?,
                index,
                limit_time: item.max_effective_time,
            })
        })
        .collect()
}

fn open_parent_dir_logged(root: &OpenDir, parent_rel: &Path) -> Option<OpenDir> {
    match root.open_descendant_dir(parent_rel) {
        Ok(opened) => opened,
        Err(err) => {
            tracing::warn!(
                parent = %parent_rel.display(),
                error = %err,
                "failed to open cached file parent",
            );
            None
        }
    }
}

fn unlink_if_unmodified(parent: &OpenDir, leaf: &OsStr, limit_time: Option<SystemTime>) -> bool {
    if let Some(limit) = limit_time {
        let Some(stat) = parent
            .child_file_stat(leaf)
            .ok()
            .flatten()
        else {
            return false;
        };
        if stat.modified.max(stat.accessed) > limit {
            return false;
        }
    }
    match parent.remove_child_file(leaf) {
        Ok(removed) => removed,
        Err(err) => {
            tracing::warn!(file = ?leaf, error = %err, "failed to remove cached file");
            false
        }
    }
}

pub(in crate::cache_maintenance) async fn prune_empty_cache_dirs(cache_dir: &Path) -> Result<()> {
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
    let root_rel = root_parent_rel.join(&root_leaf);
    if !is_protected_cache_dir(cache_dir, &cache_dir.join(&root_rel)) {
        out.push((root_parent_rel.to_path_buf(), root_leaf));
    }
    let root_entries = root.entry_names()?.into_iter();
    let mut stack: Vec<(OpenDir, IntoIter<OsString>, PathBuf)> =
        vec![(root, root_entries, root_rel)];
    while let Some((dir, iter, rel)) = stack.last_mut() {
        let Some(child_name) = iter.next() else {
            stack.pop();
            continue;
        };
        if let Some(subdir) = dir.open_child_dir(&child_name)? {
            let parent_rel = rel.clone();
            let child_rel = parent_rel.join(&child_name);
            if !is_protected_cache_dir(cache_dir, &cache_dir.join(&child_rel)) {
                out.push((parent_rel, child_name));
            }
            let child_entries = subdir.entry_names()?.into_iter();
            stack.push((subdir, child_entries, child_rel));
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
