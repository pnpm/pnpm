use super::{FindWorkspaceInventoryError, is_ignorable_discovery_error};
use cap_primitives::{ambient_authority, fs};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

#[cfg(test)]
pub(super) enum InventoryTraversalEvent<'a> {
    BeforeRead(&'a Path),
    BeforeOpenDirectory(&'a Path),
}

#[cfg(not(test))]
pub(super) struct InventoryTraversalEvent<'a> {
    _path: std::marker::PhantomData<&'a Path>,
}

impl<'a> InventoryTraversalEvent<'a> {
    fn before_read(path: &'a Path) -> Self {
        #[cfg(test)]
        {
            Self::BeforeRead(path)
        }
        #[cfg(not(test))]
        {
            let _ = path;
            Self { _path: std::marker::PhantomData }
        }
    }

    fn before_open_directory(path: &'a Path) -> Self {
        #[cfg(test)]
        {
            Self::BeforeOpenDirectory(path)
        }
        #[cfg(not(test))]
        {
            let _ = path;
            Self { _path: std::marker::PhantomData }
        }
    }
}

struct PendingDirectory {
    path: PathBuf,
    handle: std::fs::File,
}

pub(super) fn walk_workspace(
    workspace_root: &Path,
    ignored: &BTreeSet<&OsStr>,
    mut traversal_hook: impl FnMut(InventoryTraversalEvent<'_>) -> io::Result<()>,
    mut visit_file: impl FnMut(PathBuf, &OsStr),
) -> Result<(), FindWorkspaceInventoryError> {
    let root_handle =
        fs::open_ambient_dir(workspace_root, ambient_authority()).map_err(|source| {
            FindWorkspaceInventoryError::ReadDirectory {
                path: workspace_root.to_path_buf(),
                source,
            }
        })?;
    let mut pending =
        vec![PendingDirectory { path: workspace_root.to_path_buf(), handle: root_handle }];

    while let Some(directory) = pending.pop() {
        match traversal_hook(InventoryTraversalEvent::before_read(&directory.path)) {
            Ok(()) => {}
            Err(error)
                if directory.path != workspace_root && is_ignorable_discovery_error(&error) =>
            {
                continue;
            }
            Err(source) => {
                return Err(FindWorkspaceInventoryError::ReadDirectory {
                    path: directory.path,
                    source,
                });
            }
        }
        let entries = match fs::read_base_dir(&directory.handle) {
            Ok(entries) => entries,
            Err(error)
                if directory.path != workspace_root && is_ignorable_discovery_error(&error) =>
            {
                continue;
            }
            Err(source) => {
                return Err(FindWorkspaceInventoryError::ReadDirectory {
                    path: directory.path,
                    source,
                });
            }
        };
        collect_directory_entries(
            &directory,
            entries,
            ignored,
            &mut traversal_hook,
            &mut pending,
            &mut visit_file,
        )?;
    }
    Ok(())
}

fn collect_directory_entries(
    directory: &PendingDirectory,
    entries: fs::ReadDir,
    ignored: &BTreeSet<&OsStr>,
    traversal_hook: &mut impl FnMut(InventoryTraversalEvent<'_>) -> io::Result<()>,
    pending: &mut Vec<PendingDirectory>,
    visit_file: &mut impl FnMut(PathBuf, &OsStr),
) -> Result<(), FindWorkspaceInventoryError> {
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_ignorable_discovery_error(&error) => continue,
            Err(source) => {
                return Err(FindWorkspaceInventoryError::ReadEntry {
                    path: directory.path.clone(),
                    source,
                });
            }
        };
        collect_entry(directory, &entry, ignored, traversal_hook, pending, visit_file)?;
    }
    Ok(())
}

fn collect_entry(
    directory: &PendingDirectory,
    entry: &fs::DirEntry,
    ignored: &BTreeSet<&OsStr>,
    traversal_hook: &mut impl FnMut(InventoryTraversalEvent<'_>) -> io::Result<()>,
    pending: &mut Vec<PendingDirectory>,
    visit_file: &mut impl FnMut(PathBuf, &OsStr),
) -> Result<(), FindWorkspaceInventoryError> {
    let file_name = entry.file_name();
    let path = directory.path.join(&file_name);
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) if is_ignorable_discovery_error(&error) => return Ok(()),
        Err(source) => {
            return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
        }
    };
    if file_type.is_symlink() {
        return Ok(());
    }
    if file_type.is_dir() {
        collect_directory(directory, &file_name, path, ignored, traversal_hook, pending)
    } else {
        if file_type.is_file() {
            visit_file(path, &file_name);
        }
        Ok(())
    }
}

fn collect_directory(
    parent: &PendingDirectory,
    file_name: &OsStr,
    path: PathBuf,
    ignored: &BTreeSet<&OsStr>,
    traversal_hook: &mut impl FnMut(InventoryTraversalEvent<'_>) -> io::Result<()>,
    pending: &mut Vec<PendingDirectory>,
) -> Result<(), FindWorkspaceInventoryError> {
    if ignored.contains(file_name) {
        return Ok(());
    }
    traversal_hook(InventoryTraversalEvent::before_open_directory(&path)).map_err(|source| {
        FindWorkspaceInventoryError::InspectCandidate { path: path.clone(), source }
    })?;
    match fs::open_dir_nofollow(&parent.handle, Path::new(file_name)) {
        Ok(handle) => pending.push(PendingDirectory { path, handle }),
        Err(error) if is_changed_candidate_error(&error) => {}
        Err(source) => {
            return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
        }
    }
    Ok(())
}

fn is_changed_candidate_error(error: &io::Error) -> bool {
    is_ignorable_discovery_error(error)
        || error.kind() == io::ErrorKind::NotADirectory
        || is_symlink_loop(error)
}

#[cfg(unix)]
fn is_symlink_loop(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_loop(_error: &io::Error) -> bool {
    false
}
