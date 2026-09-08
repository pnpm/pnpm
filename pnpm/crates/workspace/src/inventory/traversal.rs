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

struct DirectoryFrame {
    path: PathBuf,
    identity: fs::Metadata,
    children: std::vec::IntoIter<PathBuf>,
}

/// Return the number of directory navigation opens during the scan.
pub(super) fn walk_workspace(
    workspace_root: &Path,
    ignored: &IgnoredDirectories<'_>,
    mut before_read: impl FnMut(&Path) -> io::Result<()>,
    mut before_open_directory: impl FnMut(&Path) -> io::Result<()>,
    mut visit_file: impl FnMut(PathBuf, &OsStr),
) -> Result<usize, FindWorkspaceInventoryError> {
    let root_handle =
        fs::open_ambient_dir(workspace_root, ambient_authority()).map_err(|source| {
            FindWorkspaceInventoryError::ReadDirectory {
                path: workspace_root.to_path_buf(),
                source,
            }
        })?;
    let handle = root_handle.try_clone().map_err(|source| {
        FindWorkspaceInventoryError::ReadDirectory { path: workspace_root.to_path_buf(), source }
    })?;
    let mut navigation_opens = 2;
    let mut directory = OpenDirectory { path: workspace_root.to_path_buf(), handle };
    let mut frame =
        read_frame(&directory, workspace_root, ignored, &mut before_read, &mut visit_file)?;
    let mut ancestors = Vec::new();
    loop {
        if let Some(path) = frame.children.next() {
            before_open_directory(&path).map_err(|source| {
                FindWorkspaceInventoryError::InspectCandidate { path: path.clone(), source }
            })?;
            navigation_opens += 1;
            let handle = match fs::open_dir_nofollow(
                &directory.handle,
                Path::new(path.file_name().expect("child basename")),
            ) {
                Ok(handle) => handle,
                Err(error) if is_changed_candidate_error(&error) => continue,
                Err(source) => {
                    return Err(FindWorkspaceInventoryError::InspectCandidate { path, source });
                }
            };
            ancestors.push(frame);
            directory = OpenDirectory { path, handle };
            frame =
                read_frame(&directory, workspace_root, ignored, &mut before_read, &mut visit_file)?;
        } else {
            let Some(parent) = ancestors.pop() else {
                return Ok(navigation_opens);
            };
            navigation_opens += 1;
            let handle =
                fs::open_parent_dir(&directory.handle, ambient_authority()).map_err(|source| {
                    FindWorkspaceInventoryError::InspectCandidate {
                        path: parent.path.clone(),
                        source,
                    }
                })?;
            let identity = fs::Metadata::from_file(&handle).map_err(|source| {
                FindWorkspaceInventoryError::InspectCandidate { path: parent.path.clone(), source }
            })?;
            if !same_directory(&parent.identity, &identity) {
                return Err(FindWorkspaceInventoryError::InspectCandidate {
                    path: parent.path,
                    source: io::Error::other(
                        "directory ancestry changed during workspace discovery",
                    ),
                });
            }
            directory = OpenDirectory { path: parent.path.clone(), handle };
            frame = parent;
        }
    }
}

fn read_frame(
    directory: &OpenDirectory,
    workspace_root: &Path,
    ignored: &IgnoredDirectories<'_>,
    before_read: &mut impl FnMut(&Path) -> io::Result<()>,
    visit_file: &mut impl FnMut(PathBuf, &OsStr),
) -> Result<DirectoryFrame, FindWorkspaceInventoryError> {
    let identity = fs::Metadata::from_file(&directory.handle).map_err(|source| {
        FindWorkspaceInventoryError::InspectCandidate { path: directory.path.clone(), source }
    })?;
    let mut children = Vec::new();
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
            collect_entry(directory, &entry, ignored, &mut children, visit_file)?;
        }
    }
    Ok(DirectoryFrame { path: directory.path.clone(), identity, children: children.into_iter() })
}

// Parent handles are released during descent. Validate the reopened parent before
// inspecting siblings, since a concurrent rename can change a child's ancestry.
fn same_directory(expected: &fs::Metadata, actual: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use fs::MetadataExt;
        expected.dev() == actual.dev()
            && expected.ino() == actual.ino()
            && expected.created().ok() == actual.created().ok()
    }
    #[cfg(windows)]
    {
        use fs::_WindowsByHandle;
        expected.volume_serial_number().is_some()
            && expected.volume_serial_number() == actual.volume_serial_number()
            && expected.file_index().is_some()
            && expected.file_index() == actual.file_index()
            && expected.created().ok() == actual.created().ok()
    }
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
