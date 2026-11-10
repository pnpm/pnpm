//! The directories an install builds a global virtual store package in
//! before renaming it into its slot.

use std::{
    ffi::OsStr,
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};

/// The name of an install's staging directory beside a slot's package
/// directory: `.pnpm-<token>`. A package resolves its dependencies from the
/// slot's `node_modules` there as it does once renamed. The name is short
/// because a build runs in it, and Windows starts no process whose current
/// directory is longer than `MAX_PATH`.
#[must_use]
pub fn staging_dir_name(token: &str) -> String {
    format!("{STAGING_PREFIX}{token}")
}

/// What every staging directory's name starts with. No package name does:
/// a package name never starts with a dot.
const STAGING_PREFIX: &str = ".pnpm-";

/// How long nothing in a staging directory may change before a prune takes
/// it for the leftover of an install that died. pnpm v11 installs build in
/// these directories without the store lock a prune takes, and a package
/// may wait a long time for its build, so the leftovers of crashed installs
/// are left for a day.
const ABANDONED_AFTER: Duration = Duration::from_hours(24);

/// Remove the staging directories that died installs left beside the
/// package directory `package_dir`. Returns how many went.
pub(crate) fn remove_abandoned_staging_dirs(package_dir: &Path) -> io::Result<usize> {
    let Some(parent) = package_dir.parent() else { return Ok(0) };
    let prefix = OsStr::new(STAGING_PREFIX);
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        if remove_if_abandoned(&entry?, prefix)? {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove `entry` when it is a staging directory, named with `prefix`, that
/// no install has changed for [`ABANDONED_AFTER`]. An entry that an install
/// renamed into place meanwhile is gone, and left alone.
fn remove_if_abandoned(entry: &fs::DirEntry, prefix: &OsStr) -> io::Result<bool> {
    if !entry
        .file_name()
        .as_encoded_bytes()
        .starts_with(prefix.as_encoded_bytes())
    {
        return Ok(false);
    }
    let cutoff = SystemTime::now().checked_sub(ABANDONED_AFTER).unwrap_or(SystemTime::UNIX_EPOCH);
    match changed_since(&entry.path(), cutoff) {
        Ok(false) => {}
        Ok(true) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    match fs::remove_dir_all(entry.path()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether anything in the tree at `path` changed after `cutoff`. A build
/// writes deep into its staging directory without touching the directory
/// itself, so every entry counts. Links are not followed.
fn changed_since(path: &Path, cutoff: SystemTime) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata
        .modified()
        .is_ok_and(|modified| modified > cutoff)
    {
        return Ok(true);
    }
    if !metadata.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        if changed_since(&entry?.path(), cutoff)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
