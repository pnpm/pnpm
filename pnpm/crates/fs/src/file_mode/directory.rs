#[cfg(unix)]
use super::{narrow_mode, open_directory_without_following};
use std::{
    io,
    path::{Path, PathBuf},
};

/// `create_dir_all` that, on Unix, gives each directory it creates the
/// group permission and setgid bits of the nearest ancestor that already
/// existed. Directories that were already present are not modified.
pub fn create_dir_all_inheriting_mode(dir: &Path) -> io::Result<()> {
    #[cfg(any(unix, target_os = "wasi"))]
    let template = if dir.is_dir() { None } else { nearest_existing_ancestor(dir) };
    std::fs::create_dir_all(dir)?;
    #[cfg(any(unix, target_os = "wasi"))]
    if let Some(template) = template.as_deref() {
        grant_inherited_dir_mode(dir, template)?;
    }
    Ok(())
}

/// Closest existing directory at `dir` or above it.
///
/// A relative path whose parents are all missing resolves to `.` when the
/// working directory exists. Returns `None` when nothing on the path is a
/// directory.
#[must_use]
pub fn nearest_existing_ancestor(dir: &Path) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        if current.as_os_str().is_empty() {
            let dot = PathBuf::from(".");
            return dot.is_dir().then_some(dot);
        }
        if current.is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// After a missing directory tree is created, OR `template`'s group
/// permission and setgid bits onto each new directory, stopping before
/// `template`. Nothing is added when `template` is neither group-writable
/// nor setgid. The group read and search bits come along with group-write,
/// so a restrictive umask cannot leave a new directory group-writable but
/// not searchable.
///
/// Directories that already existed are not passed in. `EPERM`, `EACCES`,
/// and `EROFS` are ignored. The root directory is never changed. `dir` must
/// be a lexical descendant of `template` with no parent traversal in the
/// descendant suffix; invalid boundaries return `InvalidInput` before changes.
#[cfg(any(unix, target_os = "wasi"))]
pub fn grant_inherited_dir_mode(dir: &Path, template: &Path) -> io::Result<()> {
    let mut current = std::path::absolute(dir)?;
    let template = std::path::absolute(template)?;
    let descendant = current
        .strip_prefix(&template)
        .map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Permission template is not an ancestor")
        })?;
    if descendant
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Permission inheritance cannot traverse a parent directory",
        ));
    }
    let Some(template_mode) = reachable_mode(&template)? else {
        return Ok(());
    };
    let extra = inherited_dir_bits(template_mode);
    if extra == 0 {
        return Ok(());
    }
    while current != template {
        add_dir_mode_bits(&current, extra)?;
        current.pop();
    }
    Ok(())
}

/// Mode of `path`, or `None` when it is missing or cannot be stated.
#[cfg(unix)]
fn reachable_mode(path: &Path) -> io::Result<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => Ok(Some(meta.permissions().mode())),
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// OR `extra` onto the mode of the directory at `path`.
///
/// The chmod goes through a descriptor opened without following a symlink,
/// so a directory entry swapped for a symlink after it was created is
/// refused rather than followed to a directory outside the store.
/// `O_DIRECTORY` refuses any other non-directory before the open can block
/// on it, as it would on a FIFO.
#[cfg(unix)]
fn add_dir_mode_bits(path: &Path, extra: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let opened =
        crate::ensure_file::retry_on_fd_pressure(|| open_directory_without_following(path));
    let dir = match opened {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return add_unreadable_dir_mode_bits(path, extra);
        }
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let mode = dir.metadata()?.permissions().mode() & 0o7777;
    let merged = mode | extra;
    if merged == mode {
        return Ok(());
    }
    match dir.set_permissions(std::fs::Permissions::from_mode(merged)) {
        Err(error) if is_unchangeable(&error) => Ok(()),
        other => other,
    }
}

/// [`add_dir_mode_bits`] for a new directory its owner cannot open, which a
/// umask that removes owner read (such as `0o477`) produces. `fchmodat` with
/// `AT_SYMLINK_NOFOLLOW` needs no read access and still refuses a symlink.
/// Only a directory this process owns is changed.
#[cfg(unix)]
fn add_unreadable_dir_mode_bits(path: &Path, extra: u32) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Ok(());
    }
    let mode = meta.mode() & 0o7777;
    let merged = mode | extra;
    if merged == mode {
        return Ok(());
    }
    match chmod_without_following(path, merged) {
        // A C library without no-follow `fchmodat` (glibc before 2.32).
        #[cfg(target_os = "linux")]
        Err(error) if error.raw_os_error() == Some(libc::EOPNOTSUPP) => {
            ignore_unchangeable(chmod_through_path_handle(path, extra))
        }
        result => ignore_unchangeable(result),
    }
}

/// Change the mode of the directory at `path` through an `O_PATH` handle,
/// which needs no read access, via its `/proc/self/fd` entry. The entry
/// resolves to the opened inode, so a symlink swapped in after the open is
/// not followed. This is how glibc 2.32 and later implement no-follow
/// `fchmodat`. Only a directory this process owns is changed.
#[cfg(target_os = "linux")]
pub(super) fn chmod_through_path_handle(path: &Path, extra: u32) -> io::Result<()> {
    use std::os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    };
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)?;
    let meta = dir.metadata()?;
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    if meta.uid() != unsafe { libc::geteuid() } {
        return Ok(());
    }
    let mode = meta.mode() & 0o7777;
    let proc_entry = format!("/proc/self/fd/{}", dir.as_raw_fd());
    std::fs::set_permissions(proc_entry, std::fs::Permissions::from_mode(mode | extra))
}

/// `fchmodat` with `AT_SYMLINK_NOFOLLOW`: changes `path`'s mode without
/// opening it and without following a symlink there.
#[cfg(unix)]
fn chmod_without_following(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mode = narrow_mode::<libc::mode_t>(mode)?;
    // SAFETY: `c_path` is a valid NUL-terminated path that outlives the call.
    let status =
        unsafe { libc::fchmodat(libc::AT_FDCWD, c_path.as_ptr(), mode, libc::AT_SYMLINK_NOFOLLOW) };
    if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(unix)]
fn ignore_unchangeable(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if is_unchangeable(&error) => Ok(()),
        other => other,
    }
}

/// The group permission and setgid bits a new directory inherits from an
/// ancestor with `template_mode`.
#[must_use]
pub fn inherited_dir_bits(template_mode: u32) -> u32 {
    if template_mode & (0o020 | 0o2000) == 0 {
        return 0;
    }
    template_mode & (0o070 | 0o2000)
}

#[cfg(any(unix, target_os = "wasi"))]
pub(super) fn is_unchangeable(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem)
}

#[cfg(target_os = "wasi")]
fn reachable_mode(path: &Path) -> io::Result<Option<u32>> {
    match crate::copy_permissions(path) {
        Ok(mode) => Ok(Some(mode)),
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "wasi")]
fn add_dir_mode_bits(path: &Path, extra: u32) -> io::Result<()> {
    let file = match crate::wasi_fs::open_nofollow(path) {
        Ok(file) => file,
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_dir() {
        return Err(io::Error::from(io::ErrorKind::NotADirectory));
    }
    let mode = crate::wasi_fs::file_mode(&file)? & 0o7777;
    if mode | extra == mode {
        return Ok(());
    }
    match crate::wasi_fs::set_file_mode(&file, mode | extra) {
        Err(error) if is_unchangeable(&error) => Ok(()),
        result => result,
    }
}
