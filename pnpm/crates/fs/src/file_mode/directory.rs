#[cfg(all(unix, not(target_os = "linux")))]
use super::narrow_mode;
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
    #[cfg(any(unix, target_os = "wasi"))]
    if let Some(template) = template.as_deref() {
        validate_permission_boundary(dir, template)?;
    }
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
    let (_current, template, descendant) = validate_permission_boundary(dir, template)?;
    #[cfg(target_os = "wasi")]
    let _ = &descendant;
    let Some(template_mode) = reachable_mode(&template)? else {
        return Ok(());
    };
    let extra = inherited_dir_bits(template_mode);
    if extra == 0 {
        return Ok(());
    }
    #[cfg(unix)]
    return grant_unix_dir_mode(&template, &descendant, extra);
    #[cfg(target_os = "wasi")]
    {
        let mut current = _current;
        while current != template {
            add_dir_mode_bits(&current, &template, extra)?;
            current.pop();
        }
        Ok(())
    }
}

#[cfg(any(unix, target_os = "wasi"))]
fn validate_permission_boundary(
    dir: &Path,
    template: &Path,
) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
    let current = std::path::absolute(dir)?;
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
    let descendant = descendant.to_path_buf();
    Ok((current, template, descendant))
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

#[cfg(unix)]
fn grant_unix_dir_mode(template: &Path, descendant: &Path, extra: u32) -> io::Result<()> {
    let mut parent = open_directory_anchor(template)?;
    for component in descendant.components() {
        let std::path::Component::Normal(name) = component else { continue };
        let opened = crate::ensure_file::retry_on_fd_pressure(|| open_directory_at(&parent, name));
        let (child, restricted) = match opened {
            Ok(opened) => opened,
            Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        add_opened_dir_mode_bits(&parent, name, &child, restricted, extra)?;
        parent = child;
    }
    Ok(())
}

#[cfg(unix)]
fn add_opened_dir_mode_bits(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
    child: &std::fs::File,
    restricted: bool,
    extra: u32,
) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = child.metadata()?;
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    if restricted && meta.uid() != unsafe { libc::geteuid() } {
        return Ok(());
    }
    let mode = meta.mode() & 0o7777;
    let merged = mode | extra;
    if merged == mode {
        return Ok(());
    }
    let result = if restricted {
        chmod_restricted_directory(parent, name, child, merged)
    } else {
        child.set_permissions(std::fs::Permissions::from_mode(merged))
    };
    match result {
        Err(error) if is_unchangeable(&error) => Ok(()),
        other => other,
    }
}

#[cfg(unix)]
fn open_directory_anchor(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(path)
    };
    match crate::ensure_file::retry_on_fd_pressure(open) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            open_restricted_directory_anchor(path)
        }
        result => result,
    }
}

#[cfg(unix)]
fn open_restricted_directory_anchor(path: &Path) -> io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let path = path_to_cstring(path)?;
    crate::ensure_file::retry_on_fd_pressure(|| {
        // SAFETY: the path is NUL-terminated and no creation mode is needed.
        let fd =
            unsafe { libc::open(path.as_ptr(), restricted_directory_flags() & !libc::O_NOFOLLOW) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: successful open transfers ownership of this descriptor.
            Ok(unsafe { std::fs::File::from_raw_fd(fd) })
        }
    })
}

#[cfg(unix)]
fn open_directory_at(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
) -> io::Result<(std::fs::File, bool)> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = path_to_cstring(Path::new(name))?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: the name is NUL-terminated and the parent descriptor stays open.
    let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    let restricted = fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::PermissionDenied;
    if restricted {
        // SAFETY: the name and parent descriptor remain valid; no creation mode is needed.
        fd = unsafe {
            libc::openat(parent.as_raw_fd(), name.as_ptr(), restricted_directory_flags())
        };
    }
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful open transfers ownership of this descriptor.
    Ok((unsafe { std::fs::File::from_raw_fd(fd) }, restricted))
}

#[cfg(unix)]
fn path_to_cstring(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

#[cfg(target_os = "linux")]
fn restricted_directory_flags() -> libc::c_int {
    libc::O_PATH | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC
}

#[cfg(target_os = "macos")]
fn restricted_directory_flags() -> libc::c_int {
    libc::O_SEARCH | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn restricted_directory_flags() -> libc::c_int {
    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC
}

#[cfg(target_os = "linux")]
fn chmod_restricted_directory(
    _parent: &std::fs::File,
    _name: &std::ffi::OsStr,
    child: &std::fs::File,
    mode: u32,
) -> io::Result<()> {
    use std::os::unix::{fs::PermissionsExt, io::AsRawFd};
    let path = format!("/proc/self/fd/{}", child.as_raw_fd());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn chmod_restricted_directory(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
    _child: &std::fs::File,
    mode: u32,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let name = path_to_cstring(Path::new(name))?;
    let mode = narrow_mode::<libc::mode_t>(mode)?;
    // SAFETY: name is NUL-terminated and parent stays open for the call.
    let status = unsafe {
        libc::fchmodat(parent.as_raw_fd(), name.as_ptr(), mode, libc::AT_SYMLINK_NOFOLLOW)
    };
    if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
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
fn add_dir_mode_bits(path: &Path, template: &Path, extra: u32) -> io::Result<()> {
    match crate::wasi_fs::grant_directory_mode_beneath(path, template, extra) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {}
        Err(error) if is_unchangeable(&error) || error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    let file = match crate::wasi_fs::open_directory_nofollow_beneath(path, template) {
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
