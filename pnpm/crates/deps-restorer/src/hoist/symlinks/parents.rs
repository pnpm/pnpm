use super::HoistedModulesDirs;
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

/// Pre-create the destination parents serially — cheap, deduplicated,
/// and a no-op for dirs that already exist — so the parallel symlink
/// pass is one syscall per link.
pub(super) fn create_parents(
    dirs: &HoistedModulesDirs<'_>,
    scope_dirs: &HashSet<PathBuf>,
) -> Result<(), crate::SymlinkPackageError> {
    if dirs.private.starts_with(dirs.public) {
        create_hoist_root(dirs.public)?;
        create_dirs_below(dirs.public, dirs.private, ParentKind::Owned)?;
    } else if dirs.public.starts_with(dirs.private) {
        create_hoist_root(dirs.private)?;
        create_dirs_below(dirs.private, dirs.public, ParentKind::Owned)?;
    } else {
        create_hoist_root(dirs.public)?;
        create_hoist_root(dirs.private)?;
    }
    for parent in scope_dirs {
        let root = if parent.starts_with(dirs.private) { dirs.private } else { dirs.public };
        create_dirs_below(root, parent, ParentKind::Owned)?;
    }
    Ok(())
}

/// What a destination parent is allowed to be.
///
/// A hoist root (`node_modules` or the virtual store's `node_modules`) and
/// the directories above it belong to the user, so a symlink there, such as
/// a shared tree linked into a CI checkout, is accepted when it leads to a
/// directory. The directories below a root belong to pnpm: a symlink there
/// would carry the links about to be created into a store package, so each
/// one must be a real directory.
#[derive(Clone, Copy)]
enum ParentKind {
    /// A hoist root or one of its ancestors: read through symlinks.
    Root,
    /// A directory below a hoist root: must not be a symlink.
    Owned,
}

/// Create `root` below its nearest existing ancestor.
fn create_hoist_root(root: &Path) -> Result<(), crate::SymlinkPackageError> {
    let existing = root
        .ancestors()
        .find(|ancestor| {
            !matches!(std::fs::metadata(ancestor), Err(error) if error.kind() == io::ErrorKind::NotFound)
        })
        .ok_or_else(|| {
            parent_dir_error(
                root,
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "workspace hoist directory has no existing ancestor",
                ),
            )
        })?;
    create_or_validate(existing, ParentKind::Root)?;
    create_dirs_below(existing, root, ParentKind::Root)
}

/// Create every directory from `root` (exclusive) down to `parent`.
fn create_dirs_below(
    root: &Path,
    parent: &Path,
    kind: ParentKind,
) -> Result<(), crate::SymlinkPackageError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|error| {
            parent_dir_error(parent, io::Error::new(io::ErrorKind::InvalidInput, error))
        })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        create_or_validate(&current, kind)?;
    }
    Ok(())
}

fn create_or_validate(dir: &Path, kind: ParentKind) -> Result<(), crate::SymlinkPackageError> {
    let shape = match probe(dir, kind) {
        Ok(shape) => shape,
        Err(error) if error.kind() == io::ErrorKind::NotFound => match std::fs::create_dir(dir) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                probe(dir, kind).map_err(|error| parent_dir_error(dir, error))?
            }
            Err(error) => return Err(parent_dir_error(dir, error)),
        },
        Err(error) => return Err(parent_dir_error(dir, error)),
    };
    if shape.is_symlink || !shape.is_dir {
        return Err(parent_dir_error(
            dir,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace hoist directory is not a real directory",
            ),
        ));
    }
    Ok(())
}

struct ParentShape {
    is_dir: bool,
    is_symlink: bool,
}

fn probe(dir: &Path, kind: ParentKind) -> io::Result<ParentShape> {
    let metadata = match kind {
        ParentKind::Root => std::fs::metadata(dir),
        ParentKind::Owned => std::fs::symlink_metadata(dir),
    };
    match metadata {
        Ok(metadata) => Ok(ParentShape {
            is_dir: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
        }),
        #[cfg(windows)]
        Err(error)
            if error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32) =>
        {
            probe_attributes(dir, kind)
        }
        Err(error) => Err(error),
    }
}

/// Read the attributes of a directory another process holds open without
/// sharing, which the metadata calls refuse. The attributes describe the
/// link itself, so a reparse point held this way passes only as a root.
#[cfg(windows)]
fn probe_attributes(dir: &Path, kind: ParentKind) -> io::Result<ParentShape> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, GetFileAttributesW,
        INVALID_FILE_ATTRIBUTES,
    };

    let path = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `path` is NUL-terminated and remains alive for the duration of the call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    Ok(ParentShape {
        is_dir: attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        is_symlink: matches!(kind, ParentKind::Owned)
            && attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

fn parent_dir_error(dir: &Path, error: io::Error) -> crate::SymlinkPackageError {
    crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error }
}
