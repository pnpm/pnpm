use super::{FindWorkspaceInventoryError, IgnoredDirectories, is_ignorable_discovery_error};
use cap_primitives::{ambient_authority, fs};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

struct OpenDirectory<'a> {
    path: PathBuf,
    handle: &'a std::fs::File,
}

/// Return the number of directory navigation opens during the scan.
pub(super) fn walk_workspace(
    workspace_root: &Path,
    ignored: &IgnoredDirectories<'_>,
    mut before_read: impl FnMut(&Path) -> io::Result<()>,
    mut before_open_directory: impl FnMut(&Path) -> io::Result<()>,
    mut visit_file: impl FnMut(PathBuf, &OsStr),
) -> Result<usize, FindWorkspaceInventoryError> {
    let root_handle = open_workspace_root(workspace_root)?;
    let mut navigation_opens = 1;
    let mut pending = Vec::new();
    read_children(
        &OpenDirectory { path: workspace_root.to_path_buf(), handle: &root_handle },
        workspace_root,
        ignored,
        &mut before_read,
        &mut visit_file,
        &mut pending,
    )?;
    while let Some(path) = pending.pop() {
        let handle = match before_open_directory(&path).and_then(|()| {
            super::open_directory::open_directory(
                &root_handle,
                path.strip_prefix(workspace_root).expect("descendant of workspace root"),
                &mut navigation_opens,
            )
        }) {
            Ok(handle) => handle,
            Err(error) if is_changed_candidate_error(&error) => continue,
            Err(source) => {
                return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
            }
        };
        read_children(
            &OpenDirectory { path, handle: &handle },
            workspace_root,
            ignored,
            &mut before_read,
            &mut visit_file,
            &mut pending,
        )?;
    }
    Ok(navigation_opens)
}

fn open_workspace_root(
    workspace_root: &Path,
) -> Result<std::fs::File, FindWorkspaceInventoryError> {
    fs::open_ambient_dir(workspace_root, ambient_authority()).map_err(|source| {
        FindWorkspaceInventoryError::ReadDirectory { path: workspace_root.to_path_buf(), source }
    })
}

fn read_children(
    directory: &OpenDirectory<'_>,
    workspace_root: &Path,
    ignored: &IgnoredDirectories<'_>,
    before_read: &mut impl FnMut(&Path) -> io::Result<()>,
    visit_file: &mut impl FnMut(PathBuf, &OsStr),
    pending: &mut Vec<PathBuf>,
) -> Result<(), FindWorkspaceInventoryError> {
    if let Some(entries) = read_directory(directory, workspace_root, before_read)? {
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
            collect_entry(directory, &entry, ignored, pending, visit_file)?;
        }
    }
    Ok(())
}

fn read_directory(
    directory: &OpenDirectory<'_>,
    workspace_root: &Path,
    before_read: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<Option<fs::ReadDir>, FindWorkspaceInventoryError> {
    match before_read(&directory.path).and_then(|()| fs::read_base_dir(directory.handle)) {
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
    directory: &OpenDirectory<'_>,
    entry: &fs::DirEntry,
    ignored: &IgnoredDirectories<'_>,
    pending: &mut Vec<PathBuf>,
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
    if file_type.is_dir() && !ignored.contains(&file_name, &path) {
        pending.push(path);
    } else if file_type.is_file() {
        visit_file(path, &file_name);
    }
    Ok(())
}

fn is_changed_candidate_error(error: &io::Error) -> bool {
    is_ignorable_discovery_error(error)
        || error.kind() == io::ErrorKind::NotADirectory
        || is_symlink_loop(error)
        || is_changed_ancestry_error(error)
}

#[cfg(unix)]
fn is_symlink_loop(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_loop(_error: &io::Error) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn is_changed_ancestry_error(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::EAGAIN | libc::EXDEV))
}

#[cfg(not(target_os = "linux"))]
fn is_changed_ancestry_error(_error: &io::Error) -> bool {
    false
}
