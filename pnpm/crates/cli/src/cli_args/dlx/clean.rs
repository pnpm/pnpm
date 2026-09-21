use super::{OsStr, Path, PathBuf, SystemTime, cache::is_expired, fs, io, remove_dirent};

/// Remove the dlx cache entries that have outlived `max_age_minutes`, along
/// with the prepare directories a superseded `pkg` link left behind.
///
/// `max_age_minutes` of zero expires every entry without inspecting it. A
/// missing `<cache_dir>/dlx` is not an error; only directories are
/// considered, so stray files directly under it survive.
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

/// Remove one dlx cache entry when its `pkg` link has outlived
/// `max_age_minutes` or it has no `pkg` link, and otherwise remove the
/// prepare directories that link no longer points at.
fn clean_entry(cache_path: &Path, max_age_minutes: u64, now: SystemTime) -> io::Result<()> {
    let Some(link) = pkg_link_metadata(cache_path)? else {
        return remove_dir_all_if_exists(cache_path);
    };
    let expired = max_age_minutes == 0
        || link.modified().is_ok_and(|mtime| is_expired(mtime, max_age_minutes, now));
    if expired {
        return remove_dir_all_if_exists(cache_path);
    }
    // A `pkg` link whose target is gone has no entry to keep, so every other
    // child is an orphan.
    let link_target = dunce::canonicalize(cache_path.join("pkg")).ok();
    for entry in fs::read_dir(cache_path)? {
        let entry = entry?;
        if entry.file_name() == OsStr::new("pkg") {
            continue;
        }
        let path = entry.path();
        if !is_link_target(&path, link_target.as_deref()) {
            remove_dirent_if_exists(&path)?;
        }
    }
    Ok(())
}

/// The metadata of the entry's `pkg` link, or `None` when it has none.
fn pkg_link_metadata(cache_path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(cache_path.join("pkg")) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `child` resolves to the directory the entry's `pkg` link points
/// at. A link whose target is gone keeps no child.
fn is_link_target(child: &Path, link_target: Option<&Path>) -> bool {
    link_target.is_some_and(|target| dunce::canonicalize(child).is_ok_and(|child| child == target))
}

/// The direct subdirectories of `dir`, or `None` when `dir` does not exist.
fn read_subdirs(dir: &Path) -> io::Result<Option<Vec<PathBuf>>> {
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

/// Remove `path` if it exists, ignoring the race with another process that
/// removed it first.
fn remove_dir_all_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// [`remove_dir_all_if_exists`] for an entry that may be a file or a symlink
/// rather than a directory.
fn remove_dirent_if_exists(path: &Path) -> io::Result<()> {
    match remove_dirent(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
