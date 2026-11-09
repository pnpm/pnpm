//! The directories an install builds a global virtual store package in
//! before renaming it into its slot.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};

/// The name of the staging directory beside a slot's package directory
/// `package_dir_name`: `.<package_dir_name>-<token>`. A package resolves its
/// dependencies from the slot's `node_modules` there as it does once renamed.
#[must_use]
pub fn staging_dir_name(package_dir_name: &OsStr, token: &str) -> OsString {
    let mut name = OsString::from(".");
    name.push(package_dir_name);
    name.push("-");
    name.push(token);
    name
}

/// How long a staging directory goes unchanged before a prune takes it for
/// the leftover of an install that died. pnpm v11 installs build into these
/// directories without the store lock a prune takes.
const ABANDONED_AFTER: Duration = Duration::from_hours(1);

/// Remove the staging directories that died installs left beside the
/// package directory `package_dir`. Returns how many went.
pub(crate) fn remove_abandoned_staging_dirs(package_dir: &Path) -> io::Result<usize> {
    let (Some(parent), Some(name)) = (package_dir.parent(), package_dir.file_name()) else {
        return Ok(0);
    };
    let prefix = staging_dir_name(name, "");
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        if !entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(prefix.as_encoded_bytes())
            || !is_abandoned(&entry.metadata()?)
        {
            continue;
        }
        match fs::remove_dir_all(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
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
