//! Expired `pnpm dlx` cache cleanup for `pnpm store prune`.
//!
//! Pacquet's port of pnpm v11's `cleanExpiredDlxCache`: without it, every
//! prepare directory a `pnpm dlx` run supersedes stays on disk forever.

use miette::{Context, IntoDiagnostic};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Remove the expired `pnpm dlx` cache entries below `<cacheDir>/dlx`.
///
/// A cache-key directory is removed wholesale when its `pkg` link is older
/// than `dlx_cache_max_age` minutes (a `0` max age removes every entry
/// without consulting mtimes); orphaned prepare directories that no link
/// points at are swept as well. A missing `dlx` directory is not an error.
pub(crate) fn clean_expired_dlx_cache(
    cache_dir: &Path,
    dlx_cache_max_age: u64,
    now: SystemTime,
) -> miette::Result<()> {
    let dlx_cache_dir = cache_dir.join("dlx");
    remove_expired_entries(&dlx_cache_dir, dlx_cache_max_age, now)?;
    sweep_orphans(&dlx_cache_dir)
}

/// Remove every cache-key directory whose `pkg` link is older than
/// `dlx_cache_max_age` minutes, or every directory when the max age is `0`.
fn remove_expired_entries(
    dlx_cache_dir: &Path,
    dlx_cache_max_age: u64,
    now: SystemTime,
) -> miette::Result<()> {
    for key_dir in key_dirs(dlx_cache_dir)? {
        if dlx_cache_max_age == 0
            || cache_link_is_outdated(&key_dir.join("pkg"), dlx_cache_max_age, now)?
        {
            remove_entry_if_exists(&key_dir, true)
                .into_diagnostic()
                .wrap_err(format!("removing expired dlx cache entry {}", key_dir.display()))?;
        }
    }
    Ok(())
}

/// Whether the `pkg` link's mtime is older than `max_age_minutes`. A
/// missing link is not outdated here: [`remove_linkless_key_dirs`] removes
/// key directories left behind without a link. A link newer than `now`
/// (clock skew) counts as fresh, matching the cache-hit check in
/// [`super::cache::get_valid_cache_dir`].
fn cache_link_is_outdated(
    cache_link: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> miette::Result<bool> {
    let meta = match fs::symlink_metadata(cache_link) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("reading dlx cache link {}", cache_link.display()));
        }
    };
    let mtime = meta
        .modified()
        .into_diagnostic()
        .wrap_err(format!("reading mtime of dlx cache link {}", cache_link.display()))?;
    let max_age = Duration::from_secs(max_age_minutes.saturating_mul(60));
    Ok(now
        .duration_since(mtime)
        .is_ok_and(|age| age > max_age))
}

/// Sweep what the expiry pass leaves behind: key directories without a
/// `pkg` link are removed entirely, and prepare directories the link no
/// longer points at are removed from the key directories that keep one.
fn sweep_orphans(dlx_cache_dir: &Path) -> miette::Result<()> {
    // Canonicalize once so the live-target comparison below survives
    // symlinked parents (`/var` -> `/private/var` on macOS tempdirs).
    let root = dunce::canonicalize(dlx_cache_dir).unwrap_or_else(|_| dlx_cache_dir.to_path_buf());
    remove_linkless_key_dirs(&root)?;
    let live = live_prepare_targets(&root)?;
    remove_orphaned_prepare_dirs(&root, &live)
}

/// The real directories directly below the dlx cache root. A missing root
/// is not an error; anything that is not a directory is ignored.
fn key_dirs(dlx_cache_dir: &Path) -> miette::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dlx_cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("reading the dlx cache directory {}", dlx_cache_dir.display()));
        }
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry
            .into_diagnostic()
            .wrap_err(format!(
                "reading an entry of the dlx cache directory {}",
                dlx_cache_dir.display(),
            ))?;
        let path = entry.path();
        if entry
            .file_type()
            .into_diagnostic()
            .wrap_err(format!("inspecting dlx cache entry {}", path.display()))?
            .is_dir()
        {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

/// Remove the key directories that lost their `pkg` link.
fn remove_linkless_key_dirs(dlx_cache_dir: &Path) -> miette::Result<()> {
    for key_dir in key_dirs(dlx_cache_dir)? {
        let cache_link = key_dir.join("pkg");
        match fs::symlink_metadata(&cache_link) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                remove_entry_if_exists(&key_dir, true)
                    .into_diagnostic()
                    .wrap_err(format!("removing orphaned dlx cache entry {}", key_dir.display()))?;
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err(format!("reading dlx cache link {}", cache_link.display()));
            }
        }
    }
    Ok(())
}

/// The canonical prepare directories the live `pkg` links still point at.
/// A missing or dangling link names nothing to keep.
fn live_prepare_targets(dlx_cache_dir: &Path) -> miette::Result<HashSet<PathBuf>> {
    let mut live = HashSet::new();
    for key_dir in key_dirs(dlx_cache_dir)? {
        let cache_link = key_dir.join("pkg");
        match dunce::canonicalize(&cache_link) {
            Ok(target) => {
                live.insert(target);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err(format!("resolving dlx cache link {}", cache_link.display()));
            }
        }
    }
    Ok(live)
}

/// Remove every prepare directory that is not the live `pkg` link target.
/// The link itself is never touched.
fn remove_orphaned_prepare_dirs(
    dlx_cache_dir: &Path,
    live: &HashSet<PathBuf>,
) -> miette::Result<()> {
    for key_dir in key_dirs(dlx_cache_dir)? {
        let children = fs::read_dir(&key_dir)
            .into_diagnostic()
            .wrap_err(format!("reading dlx cache entry {}", key_dir.display()))?;
        for child in children {
            let child = child
                .into_diagnostic()
                .wrap_err(format!("reading an entry of dlx cache entry {}", key_dir.display()))?;
            remove_orphan_child(&child, live)?;
        }
    }
    Ok(())
}

/// Remove one entry of a cache-key directory unless it is the `pkg` link
/// itself or the prepare directory the link points at.
fn remove_orphan_child(child: &fs::DirEntry, live: &HashSet<PathBuf>) -> miette::Result<()> {
    if child.file_name().as_os_str() == "pkg" {
        return Ok(());
    }
    let child_path = child.path();
    // A symlink must be judged (and deleted) as the link itself: canonicalizing
    // it would resolve to the target and the removal below would delete a file
    // or directory outside the cache instead of the orphaned link.
    let file_type = child
        .file_type()
        .into_diagnostic()
        .wrap_err(format!("inspecting dlx cache entry {}", child_path.display()))?;
    if file_type.is_symlink() {
        return remove_entry_if_exists(&child_path, false)
            .into_diagnostic()
            .wrap_err(format!("removing orphaned dlx cache entry {}", child_path.display()));
    }
    let canonical = dunce::canonicalize(&child_path).unwrap_or_else(|_| child_path.clone());
    if live.contains(&canonical) {
        return Ok(());
    }
    remove_entry_if_exists(&canonical, file_type.is_dir())
        .into_diagnostic()
        .wrap_err(format!("removing orphaned dlx cache entry {}", canonical.display()))
}

/// Remove a file or directory, tolerating a concurrent deleter winning the
/// race.
fn remove_entry_if_exists(path: &Path, is_dir: bool) -> io::Result<()> {
    let removed = if is_dir { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
