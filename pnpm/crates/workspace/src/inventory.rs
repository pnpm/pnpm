use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

/// Manifest paths found by one repository traversal.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkspaceInventory {
    manifests: BTreeMap<String, Vec<PathBuf>>,
}

impl WorkspaceInventory {
    /// Paths whose final component equals `basename`.
    pub fn manifests(&self, basename: &str) -> &[PathBuf] {
        self.manifests.get(basename).map_or(&[], Vec::as_slice)
    }
}

/// Error returned while building a [`WorkspaceInventory`].
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to walk workspace inventory under {}: {source}", root.display())]
#[diagnostic(code(ERR_PNPM_WORKSPACE_INVENTORY_WALK_ERROR))]
pub struct FindWorkspaceInventoryError {
    root: PathBuf,
    #[error(source)]
    source: io::Error,
}

/// Find the requested manifest basenames with one recursive traversal.
///
/// Directory symlinks are not followed. An entry that disappears or becomes
/// unreadable during a nested traversal is skipped, while failure to read the
/// inventory root is reported.
pub fn find_workspace_inventory(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    find_workspace_inventory_with(
        workspace_root,
        manifest_basenames,
        ignored_directory_basenames,
        |directory| fs::read_dir(directory),
    )
}

fn find_workspace_inventory_with(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
    mut read_dir: impl FnMut(&Path) -> io::Result<fs::ReadDir>,
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    let requested: BTreeSet<&OsStr> = manifest_basenames.iter().map(OsStr::new).collect();
    let ignored: BTreeSet<&OsStr> = ignored_directory_basenames.iter().map(OsStr::new).collect();
    let mut manifests: BTreeMap<String, Vec<PathBuf>> =
        manifest_basenames.iter().map(|basename| ((*basename).to_string(), Vec::new())).collect();
    let mut pending = vec![workspace_root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let entries = match read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory != workspace_root && is_ignorable_discovery_error(&error) => {
                continue;
            }
            Err(source) => {
                return Err(workspace_inventory_error(workspace_root, source));
            }
        };
        for entry in entries {
            match entry.and_then(|entry| {
                collect_inventory_entry(&entry, &requested, &ignored, &mut pending, &mut manifests)
            }) {
                Ok(()) => {}
                Err(error) if is_ignorable_discovery_error(&error) => continue,
                Err(source) => {
                    return Err(workspace_inventory_error(workspace_root, source));
                }
            }
        }
    }

    for manifest_paths in manifests.values_mut() {
        manifest_paths.sort();
    }
    Ok(WorkspaceInventory { manifests })
}

fn collect_inventory_entry(
    entry: &fs::DirEntry,
    requested: &BTreeSet<&OsStr>,
    ignored: &BTreeSet<&OsStr>,
    pending: &mut Vec<PathBuf>,
    manifests: &mut BTreeMap<String, Vec<PathBuf>>,
) -> io::Result<()> {
    let file_type = entry.file_type()?;
    if file_type.is_symlink() {
        return Ok(());
    }
    let file_name = entry.file_name();
    if file_type.is_dir() {
        if !ignored.contains(file_name.as_os_str()) {
            pending.push(entry.path());
        }
    } else if file_type.is_file()
        && requested.contains(file_name.as_os_str())
        && let Some(manifest_paths) =
            file_name.to_str().and_then(|basename| manifests.get_mut(basename))
    {
        manifest_paths.push(entry.path());
    }
    Ok(())
}

fn workspace_inventory_error(
    workspace_root: &Path,
    source: io::Error,
) -> FindWorkspaceInventoryError {
    FindWorkspaceInventoryError { root: workspace_root.to_path_buf(), source }
}

fn is_ignorable_discovery_error(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
mod tests;
