use pnpm_fs::{is_symlink_or_junction, symlink_metadata_with_retry};

use super::{OsStr, Path, PathBuf, SystemTime, cache::is_expired, fs, io, remove_dirent};

/// Reclaim the dlx cache entries that have outlived `max_age_minutes`.
///
/// Each directory is judged by its own mtime, because `pkg` is repointed only
/// after a `pnpm dlx` install finishes: the prepare directory a concurrent run
/// is still filling has to survive the sweep.
pub(crate) fn clean_expired_dlx_cache(
    cache_dir: &Path,
    max_age_minutes: u64,
    now: SystemTime,
) -> io::Result<()> {
    let dlx_cache_dir = cache_dir.join("dlx");
    let Some(cache_paths) = read_subdirs(&dlx_cache_dir)? else {
        return Ok(());
    };
    for cache_path in &cache_paths {
        clean_entry(cache_path, max_age_minutes, now)?;
    }
    Ok(())
}

fn clean_entry(cache_path: &Path, max_age_minutes: u64, now: SystemTime) -> io::Result<()> {
    let entry_expired = is_reclaimable(cache_path, max_age_minutes, now)?;
    let Some(link) = pkg_link(cache_path)? else {
        if entry_expired {
            remove_dir_all_if_exists(cache_path)?;
        }
        return Ok(());
    };
    let link_expired = link.modified().is_ok_and(|mtime| is_stale(mtime, max_age_minutes, now));
    if entry_expired && link_expired {
        return remove_dir_all_if_exists(cache_path);
    }
    clean_orphans(cache_path, max_age_minutes, now)
}

fn is_reclaimable(path: &Path, max_age_minutes: u64, now: SystemTime) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            Ok(metadata.modified().is_ok_and(|mtime| is_stale(mtime, max_age_minutes, now)))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_stale(mtime: SystemTime, max_age_minutes: u64, now: SystemTime) -> bool {
    max_age_minutes == 0 || is_expired(mtime, max_age_minutes, now)
}

fn pkg_link(cache_path: &Path) -> io::Result<Option<fs::Metadata>> {
    let link = cache_path.join("pkg");
    match symlink_metadata_with_retry(&link) {
        Ok(metadata) if is_symlink_or_junction(&link)? => Ok(Some(metadata)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn clean_orphans(cache_path: &Path, max_age_minutes: u64, now: SystemTime) -> io::Result<()> {
    let link_target = dunce::canonicalize(cache_path.join("pkg")).ok();
    for entry in fs::read_dir(cache_path)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("pkg") {
            continue;
        }
        let path = entry.path();
        if !is_link_target(&path, link_target.as_deref())
            && is_reclaimable(&path, max_age_minutes, now)?
        {
            remove_dirent_if_exists(&path)?;
        }
    }
    Ok(())
}

fn is_link_target(child: &Path, link_target: Option<&Path>) -> bool {
    link_target.is_some_and(|target| dunce::canonicalize(child).is_ok_and(|child| child == target))
}

fn read_subdirs(dir: &Path) -> io::Result<Option<Vec<PathBuf>>> {
    // A `dlx` root that is a link would take the sweep out of the cache.
    match is_symlink_or_junction(dir) {
        Ok(true) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(false) => {}
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            paths.push(entry.path());
        }
    }
    Ok(Some(paths))
}

fn remove_dir_all_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn remove_dirent_if_exists(path: &Path) -> io::Result<()> {
    match remove_dirent(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
