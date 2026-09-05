use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

mod traversal;

use traversal::walk_workspace;

/// Manifest paths grouped by basename.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkspaceInventory {
    manifests: BTreeMap<String, Vec<PathBuf>>,
}

impl WorkspaceInventory {
    /// Paths whose final component equals `basename`.
    pub fn manifests(&self, basename: &str) -> Option<&[PathBuf]> {
        self.manifests.get(basename).map(Vec::as_slice)
    }
}

/// Error returned while building a [`WorkspaceInventory`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindWorkspaceInventoryError {
    #[display("Failed to read workspace inventory directory {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_INVENTORY_READ_DIRECTORY))]
    ReadDirectory {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to read an entry in workspace inventory directory {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_INVENTORY_READ_ENTRY))]
    ReadEntry {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to inspect workspace inventory candidate {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_INVENTORY_INSPECT_CANDIDATE))]
    InspectCandidate {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

/// Find paths whose final components match the requested manifest basenames.
///
/// Directory symlinks are not followed. An entry that disappears or becomes
/// unreadable during a nested traversal is skipped, while failure to read the
/// inventory root is reported.
/// Ignored directories may be absolute or relative to the inventory root.
pub fn find_workspace_inventory(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
    ignored_directories: &[PathBuf],
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    find_workspace_inventory_with(
        workspace_root,
        manifest_basenames,
        ignored_directory_basenames,
        ignored_directories,
        |_| Ok(()),
        |_| Ok(()),
    )
}

fn find_workspace_inventory_with(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
    ignored_directories: &[PathBuf],
    before_read: impl FnMut(&Path) -> io::Result<()>,
    before_open_directory: impl FnMut(&Path) -> io::Result<()>,
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    let requested: BTreeSet<&OsStr> = manifest_basenames.iter().map(OsStr::new).collect();
    let canonical_root = fs::canonicalize(workspace_root).map_err(|source| {
        FindWorkspaceInventoryError::ReadDirectory { path: workspace_root.to_path_buf(), source }
    })?;
    let mut paths = BTreeSet::new();
    for path in ignored_directories {
        let path = workspace_root.join(path);
        let canonical = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
            }
        };
        if let Ok(relative) = canonical.strip_prefix(&canonical_root) {
            paths.insert(relative.to_path_buf());
        }
    }
    let ignored = IgnoredDirectories {
        root: workspace_root,
        basenames: ignored_directory_basenames.iter().map(OsStr::new).collect(),
        paths,
    };
    let mut manifests: BTreeMap<String, Vec<PathBuf>> =
        manifest_basenames.iter().map(|basename| ((*basename).to_string(), Vec::new())).collect();

    walk_workspace(
        workspace_root,
        &ignored,
        before_read,
        before_open_directory,
        |path, file_name| {
            if requested.contains(file_name)
                && let Some(manifest_paths) =
                    file_name.to_str().and_then(|basename| manifests.get_mut(basename))
            {
                manifest_paths.push(path);
            }
        },
    )?;

    for manifest_paths in manifests.values_mut() {
        manifest_paths.sort();
    }
    Ok(WorkspaceInventory { manifests })
}

struct IgnoredDirectories<'a> {
    root: &'a Path,
    basenames: BTreeSet<&'a OsStr>,
    paths: BTreeSet<PathBuf>,
}

impl IgnoredDirectories<'_> {
    fn contains(&self, basename: &OsStr, path: &Path) -> bool {
        self.basenames.contains(basename)
            || path.strip_prefix(self.root).is_ok_and(|relative| self.paths.contains(relative))
    }
}

fn is_ignorable_discovery_error(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
mod tests;
