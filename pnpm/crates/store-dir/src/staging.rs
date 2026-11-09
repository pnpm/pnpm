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

/// How long a staging directory goes unchanged before a prune takes it for
/// the leftover of an install that died. pnpm v11 installs build into these
/// directories without the store lock a prune takes.
const ABANDONED_AFTER: Duration = Duration::from_hours(1);

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
    let metadata = match entry.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !is_abandoned(&metadata) {
        return Ok(false);
    }
    match fs::remove_dir_all(entry.path()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_abandoned(metadata: &fs::Metadata) -> bool {
    metadata.is_dir()
        && metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age >= ABANDONED_AFTER)
}

#[cfg(test)]
mod tests;
