use super::HoistedModulesDirs;
use std::{collections::HashSet, path::PathBuf};

/// Pre-create the destination parents serially — cheap, deduplicated,
/// and a no-op for dirs that already exist — so the parallel symlink
/// pass is one syscall per link.
pub(super) fn create_parents(
    dirs: &HoistedModulesDirs<'_>,
    scope_dirs: &HashSet<PathBuf>,
) -> Result<(), crate::SymlinkPackageError> {
    if dirs.private.starts_with(dirs.public) {
        create_hoist_root_from_ancestor(dirs.public)?;
        create_hoist_scope_dirs(dirs.public, dirs.private)?;
    } else if dirs.public.starts_with(dirs.private) {
        create_hoist_root_from_ancestor(dirs.private)?;
        create_hoist_scope_dirs(dirs.private, dirs.public)?;
    } else {
        create_hoist_root_from_ancestor(dirs.public)?;
        create_hoist_root_from_ancestor(dirs.private)?;
    }
    for parent in scope_dirs {
        let root = if parent.starts_with(dirs.private) { dirs.private } else { dirs.public };
        create_hoist_scope_dirs(root, parent)?;
    }
    Ok(())
}

fn create_hoist_root_from_ancestor(
    root: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let trusted_root = root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Ok)
        .unwrap_or_else(|| filesystem_root(root))?;
    create_hoist_root(trusted_root, root)
}

fn filesystem_root(path: &std::path::Path) -> Result<&std::path::Path, crate::SymlinkPackageError> {
    path.ancestors()
        .last()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .ok_or_else(|| crate::SymlinkPackageError::CreateParentDir {
            dir: path.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workspace hoist directory must be absolute",
            ),
        })
}

fn create_hoist_root(
    trusted_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match create_or_validate_hoist_parent(trusted_root)
        .and_then(|()| create_hoist_parent_dirs(trusted_root, root))
    {
        Ok(()) => return Ok(()),
        Err(crate::SymlinkPackageError::CreateParentDir { error, .. })
            if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let existing_root = trusted_root
        .ancestors()
        .skip(1)
        .find(|ancestor| match std::fs::metadata(ancestor) {
            Ok(_) => true,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
        .ok_or_else(|| crate::SymlinkPackageError::CreateParentDir {
            dir: trusted_root.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "workspace hoist directory has no existing ancestor",
            ),
        })?;
    create_or_validate_hoist_parent(existing_root)?;
    create_hoist_parent_dirs(existing_root, root)
}

fn create_hoist_parent_dirs(
    root: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: parent.to_path_buf(),
            error: std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
        })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        create_or_validate_hoist_parent(&current)?;
    }
    Ok(())
}

fn create_hoist_scope_dirs(
    root: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: parent.to_path_buf(),
            error: std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
        })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        create_or_validate_scope_parent(&current)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_or_validate_hoist_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match std::fs::metadata(dir) {
        Ok(_) => validate_real_hoist_dir(dir),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_real_hoist_dir(dir)
                }
                Err(error) => Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                }),
            }
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(not(windows))]
fn create_or_validate_scope_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match std::fs::symlink_metadata(dir) {
        Ok(_) => validate_real_scope_dir(dir),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_real_scope_dir(dir)
                }
                Err(error) => Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                }),
            }
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(windows)]
fn create_or_validate_hoist_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match std::fs::metadata(dir) {
        Ok(_) => validate_real_hoist_dir(dir),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_real_hoist_dir(dir)
                }
                Err(error) => Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                }),
            }
        }
        Err(_) => validate_real_hoist_dir(dir),
    }
}

#[cfg(windows)]
fn create_or_validate_scope_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match windows_file_attributes(dir) {
        Ok(attributes) => validate_real_scope_dir(dir, attributes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    windows_file_attributes(dir)
                        .and_then(|attr| validate_real_scope_dir_attr(dir, attr))
                        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
                            dir: dir.to_path_buf(),
                            error,
                        })
                }
                Err(error) => Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                }),
            }
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(not(windows))]
fn validate_real_hoist_dir(dir: &std::path::Path) -> Result<(), crate::SymlinkPackageError> {
    let metadata = std::fs::metadata(dir)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error,
        })?;
    if !metadata.is_dir() {
        return Err(crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workspace hoist directory is not a real directory",
            ),
        });
    }
    Ok(())
}

#[cfg(not(windows))]
fn validate_real_scope_dir(dir: &std::path::Path) -> Result<(), crate::SymlinkPackageError> {
    let metadata = std::fs::symlink_metadata(dir)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workspace hoist directory is not a real directory",
            ),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn windows_file_attributes(dir: &std::path::Path) -> std::io::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};

    match std::fs::symlink_metadata(dir) {
        Ok(metadata) => return Ok(metadata.file_attributes()),
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {}
        Err(error) => return Err(error),
    }
    let path = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `path` is NUL-terminated and remains alive for the duration of the call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(attributes)
    }
}

#[cfg(windows)]
fn validate_real_hoist_dir(dir: &std::path::Path) -> Result<(), crate::SymlinkPackageError> {
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

    match std::fs::metadata(dir) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "workspace hoist directory is not a real directory",
                    ),
                });
            }
            Ok(())
        }
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {
            let attributes = windows_file_attributes(dir)
                .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                })?;
            if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                return Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "workspace hoist directory is not a real directory",
                    ),
                });
            }
            Ok(())
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(windows)]
fn validate_real_scope_dir(
    dir: &std::path::Path,
    attributes: u32,
) -> Result<(), crate::SymlinkPackageError> {
    validate_real_scope_dir_attr(dir, attributes)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error,
        })
}

#[cfg(windows)]
fn validate_real_scope_dir_attr(_dir: &std::path::Path, attributes: u32) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace hoist directory is not a real directory",
        ));
    }
    Ok(())
}
