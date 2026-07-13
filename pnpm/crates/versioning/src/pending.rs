use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{error::VersioningError, intents::CHANGES_DIR};

/// Directory (under the workspace's `.changeset/`) holding the composed
/// changelog sections of releases whose intents are not yet published. In
/// `registry` changelog storage no CHANGELOG.md is committed, so a release's
/// section is parked here at `pnpm version -r` time and consumed at publish,
/// when it is prepended to the previously published tarball's changelog. Each
/// file is garbage-collected together with the intents it was composed from,
/// under the same registry-confirmed gate. Mirrors the TypeScript
/// `pendingChangelog` module.
pub const PENDING_CHANGELOGS_DIR: &str = "changelogs";

/// A published `package@version` names one artifact, so it is a stable key for
/// its parked section. The only character in the key that a filesystem rejects
/// is the `/` of a scoped name, encoded here as `!` (a character neither a
/// package name nor a semver version can contain, so the mapping is
/// reversible).
fn pending_changelog_filename(pkg_name: &str, version: &str) -> String {
    format!("{}.md", format!("{pkg_name}@{version}").replace('/', "!"))
}

#[must_use]
pub fn pending_changelog_path(workspace_dir: &Path, pkg_name: &str, version: &str) -> PathBuf {
    workspace_dir
        .join(CHANGES_DIR)
        .join(PENDING_CHANGELOGS_DIR)
        .join(pending_changelog_filename(pkg_name, version))
}

pub fn write_pending_changelog(
    workspace_dir: &Path,
    pkg_name: &str,
    version: &str,
    section: &str,
) -> Result<(), VersioningError> {
    let path = pending_changelog_path(workspace_dir, pkg_name, version);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| VersioningError::Write { path: parent.to_path_buf(), source })?;
    }
    fs::write(&path, section).map_err(|source| VersioningError::Write { path, source })
}

pub fn read_pending_changelog(
    workspace_dir: &Path,
    pkg_name: &str,
    version: &str,
) -> Result<Option<String>, VersioningError> {
    let path = pending_changelog_path(workspace_dir, pkg_name, version);
    match fs::read_to_string(&path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(VersioningError::Read { path, source }),
    }
}

/// Removes a parked section. A missing file is not an error — it may already
/// have been collected.
pub fn remove_pending_changelog(
    workspace_dir: &Path,
    pkg_name: &str,
    version: &str,
) -> Result<(), VersioningError> {
    let path = pending_changelog_path(workspace_dir, pkg_name, version);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(VersioningError::Remove { path, source }),
    }
}
