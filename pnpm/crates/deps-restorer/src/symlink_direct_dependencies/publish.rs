use std::{
    fs, io,
    path::{Path, PathBuf},
};

use pnpm_lockfile::ProjectSnapshot;

use super::SymlinkDirectDependenciesError;

/// Removes `<publishDirectory>/<modules dir name>` when it is a link that
/// resolves to `modules_dir`, as pnpm 12.7.0 created for
/// `publishConfig.linkDirectory`. A build tool that cleans the publish
/// directory through that link deletes the dependencies' files. Real
/// directories and links to anything else are left alone.
pub(super) fn remove_publish_modules_link(
    project_snapshot: &ProjectSnapshot,
    project_dir: &Path,
    modules_dir: &Path,
) -> Result<(), SymlinkDirectDependenciesError> {
    let Some(publish_dir) = &project_snapshot.publish_directory else {
        return Ok(());
    };
    let Some(modules_name) = modules_dir.file_name() else {
        return Ok(());
    };
    let publish_dir = pnpm_fs::lexical_normalize(&project_dir.join(publish_dir));
    if !publish_dir.starts_with(project_dir) || publish_dir == project_dir {
        return Ok(());
    }
    let link = publish_dir.join(modules_name);
    let error = |source| SymlinkDirectDependenciesError::RemovePublishModulesLink {
        link: link.clone(),
        source,
    };
    let is_link = match pnpm_fs::is_symlink_or_junction(&link) {
        Ok(is_link) => is_link,
        Err(source) if source.kind() == io::ErrorKind::NotFound => false,
        Err(source) => return Err(error(source)),
    };
    if !is_link || !resolves_to(&link, modules_dir).map_err(error)? {
        return Ok(());
    }
    pnpm_fs::remove_symlink_dir(&link).map_err(error)
}

fn resolves_to(link: &Path, dir: &Path) -> io::Result<bool> {
    let Some(target) = canonicalize_existing(link)? else {
        return Ok(false);
    };
    Ok(canonicalize_existing(dir)?.is_some_and(|dir| dir == target))
}

fn canonicalize_existing(path: &Path) -> io::Result<Option<PathBuf>> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
