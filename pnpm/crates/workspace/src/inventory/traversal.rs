use super::{FindWorkspaceInventoryError, IgnoredDirectories, is_ignorable_discovery_error};
use cap_primitives::{ambient_authority, fs};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

struct OpenDirectory {
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
    let mut pending = vec![workspace_root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path != workspace_root {
            before_open_directory(&path).map_err(|source| {
                FindWorkspaceInventoryError::InspectCandidate { path: path.clone(), source }
            })?;
        }
        let relative = path.strip_prefix(workspace_root).expect("inventory paths share the root");
        let handle = match open_descendant(&root_handle, relative) {
            Ok(handle) => handle,
            Err(error) if path != workspace_root && is_changed_candidate_error(&error) => continue,
            Err(source) => {
                return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
            }
        };
        let directory = OpenDirectory { path, handle };
        let Some(entries) = read_directory(&directory, workspace_root, &mut before_read)? else {
            continue;
        };
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
            collect_entry(&directory, &entry, ignored, &mut pending, &mut visit_file)?;
        }
    }
    Ok(())
}

// Reopen queued paths from the pinned root one component at a time. This bounds
// open handles independently of tree width and depth without following symlinks.
fn open_descendant(root: &std::fs::File, relative: &Path) -> io::Result<std::fs::File> {
    let mut directory = None;
    for component in relative.components() {
        let parent = directory.as_ref().unwrap_or(root);
        directory = Some(fs::open_dir_nofollow(parent, Path::new(component.as_os_str()))?);
    }
    directory.map_or_else(|| root.try_clone(), Ok)
}

fn read_directory(
    directory: &OpenDirectory,
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
    directory: &OpenDirectory,
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
}

#[cfg(unix)]
fn is_symlink_loop(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_loop(_error: &io::Error) -> bool {
    false
}
