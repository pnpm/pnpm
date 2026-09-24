use std::path::Path;

use pnpm_lockfile::ProjectSnapshot;

use super::SymlinkDirectDependenciesError;
use crate::symlink_package::symlink_package;

pub(super) fn link_publish_modules_dir(
    importer_id: &str,
    project_snapshot: &ProjectSnapshot,
    project_dir: &Path,
    modules_dir: &Path,
) -> Result<(), SymlinkDirectDependenciesError> {
    if let Some(publish_dir) = &project_snapshot.publish_directory
        && project_snapshot.link_directory != Some(false)
        && modules_dir
            .try_exists()
            .map_err(|source| SymlinkDirectDependenciesError::InspectModulesDir {
                dir: modules_dir.to_path_buf(),
                source,
            })?
    {
        let target_dir = pnpm_fs::lexical_normalize(&project_dir.join(publish_dir));
        if !target_dir.starts_with(project_dir) || target_dir == project_dir {
            return Ok(());
        }
        let modules_name =
            modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
        let publish_modules_dir = target_dir.join(modules_name);
        symlink_package(modules_dir, &publish_modules_dir)
            .map_err(|source| SymlinkDirectDependenciesError::SymlinkPackage {
                importer_id: importer_id.to_string(),
                name: modules_name.to_string_lossy().into_owned(),
                source,
            })?;
    }
    Ok(())
}
