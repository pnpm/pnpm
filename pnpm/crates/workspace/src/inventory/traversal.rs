use super::{FindWorkspaceInventoryError, IgnoredDirectories, is_ignorable_discovery_error};
use cap_primitives::{ambient_authority, fs};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

struct PendingDirectory {
    path: PathBuf,
    handle: std::fs::File,
}

pub(super) fn walk_workspace(
    workspace_root: &Path,
    ignored: &IgnoredDirectories<'_>,
    mut before_read: impl FnMut(&Path) -> io::Result<()>,
    mut before_open_directory: impl FnMut(&Path) -> io::Result<()>,
    mut visit_file: impl FnMut(PathBuf, &OsStr),
) -> Result<(), FindWorkspaceInventoryError> {
    let root_handle =
        fs::open_ambient_dir(workspace_root, ambient_authority()).map_err(|source| {
            FindWorkspaceInventoryError::ReadDirectory {
                path: workspace_root.to_path_buf(),
                source,
            }
        })?;
    let root = PendingDirectory { path: workspace_root.to_path_buf(), handle: root_handle };
    let Some(entries) = read_directory(&root, workspace_root, &mut before_read)? else {
        return Ok(());
    };
    let mut pending = vec![(root, entries)];
    while let Some((directory, entries)) = pending.last_mut() {
        let Some(entry) = entries.next() else {
            pending.pop();
            continue;
        };
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
        if let Some(child) =
            collect_entry(directory, &entry, ignored, &mut before_open_directory, &mut visit_file)?
            && let Some(entries) = read_directory(&child, workspace_root, &mut before_read)?
        {
            pending.push((child, entries));
        }
    }
    Ok(())
}

fn read_directory(
    directory: &PendingDirectory,
    workspace_root: &Path,
    before_read: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<Option<fs::ReadDir>, FindWorkspaceInventoryError> {
    match before_read(&directory.path).and_then(|()| fs::read_base_dir(&directory.handle)) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if directory.path != workspace_root && is_ignorable_discovery_error(&error) => {
            Ok(None)
        }
        Err(source) => {
            Err(FindWorkspaceInventoryError::ReadDirectory { path: directory.path.clone(), source })
        }
    }
}

fn collect_entry(
    directory: &PendingDirectory,
    entry: &fs::DirEntry,
    ignored: &IgnoredDirectories<'_>,
    before_open_directory: &mut impl FnMut(&Path) -> io::Result<()>,
    visit_file: &mut impl FnMut(PathBuf, &OsStr),
) -> Result<Option<PendingDirectory>, FindWorkspaceInventoryError> {
    let file_name = entry.file_name();
    let path = directory.path.join(&file_name);
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) if is_ignorable_discovery_error(&error) => return Ok(None),
        Err(source) => {
            return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
        }
    };
    if file_type.is_symlink() {
        return Ok(None);
    }
    if file_type.is_dir() {
        collect_directory(directory, &file_name, path, ignored, before_open_directory)
    } else {
        if file_type.is_file() {
            visit_file(path, &file_name);
        }
        Ok(None)
    }
}

fn collect_directory(
    parent: &PendingDirectory,
    file_name: &OsStr,
    path: PathBuf,
    ignored: &IgnoredDirectories<'_>,
    before_open_directory: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<Option<PendingDirectory>, FindWorkspaceInventoryError> {
    if ignored.contains(file_name, &path) {
        return Ok(None);
    }
    before_open_directory(&path).map_err(|source| {
        FindWorkspaceInventoryError::InspectCandidate { path: path.clone(), source }
    })?;
    match fs::open_dir_nofollow(&parent.handle, Path::new(file_name)) {
        Ok(handle) => Ok(Some(PendingDirectory { path, handle })),
        Err(error) if is_changed_candidate_error(&error) => Ok(None),
        Err(source) => Err(FindWorkspaceInventoryError::InspectCandidate { path, source }),
    }
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
