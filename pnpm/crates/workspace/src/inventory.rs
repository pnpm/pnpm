use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

mod traversal;

use traversal::{InventoryTraversalEvent, walk_workspace};

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
pub fn find_workspace_inventory(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    find_workspace_inventory_with(
        workspace_root,
        manifest_basenames,
        ignored_directory_basenames,
        |_| Ok(()),
    )
}

fn find_workspace_inventory_with(
    workspace_root: &Path,
    manifest_basenames: &[&str],
    ignored_directory_basenames: &[&str],
    traversal_hook: impl FnMut(InventoryTraversalEvent<'_>) -> io::Result<()>,
) -> Result<WorkspaceInventory, FindWorkspaceInventoryError> {
    let requested: BTreeSet<&OsStr> = manifest_basenames.iter().map(OsStr::new).collect();
    let ignored: BTreeSet<&OsStr> = ignored_directory_basenames.iter().map(OsStr::new).collect();
    let mut manifests: BTreeMap<String, Vec<PathBuf>> =
        manifest_basenames.iter().map(|basename| ((*basename).to_string(), Vec::new())).collect();

    walk_workspace(workspace_root, &ignored, traversal_hook, |path, file_name| {
        if requested.contains(file_name)
            && let Some(manifest_paths) =
                file_name.to_str().and_then(|basename| manifests.get_mut(basename))
        {
            manifest_paths.push(path);
        }
    })?;

    for manifest_paths in manifests.values_mut() {
        manifest_paths.sort();
    }
    Ok(WorkspaceInventory { manifests })
}

fn is_ignorable_discovery_error(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
mod tests;
