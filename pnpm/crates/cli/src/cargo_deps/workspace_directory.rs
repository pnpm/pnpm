#[cfg(windows)]
pub(super) use windows::ensure_workspace_directory_windows;

use super::{IntoDiagnostic, Path, PathBuf, Result, fs, io};
use miette::WrapErr;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
static MANAGED_TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct ManagedDirectory {
    pub(super) path: PathBuf,
    #[cfg(unix)]
    pub(super) handle: fs::File,
    #[cfg(windows)]
    pub(super) _pinned_components: Vec<fs::File>,
}

pub(super) fn ensure_workspace_directory(
    root_dir: &Path,
    components: &[&str],
) -> Result<ManagedDirectory> {
    #[cfg(unix)]
    {
        ensure_workspace_directory_unix(root_dir.to_path_buf(), components)
    }
    #[cfg(windows)]
    {
        let root = fs::canonicalize(root_dir).into_diagnostic().wrap_err_with(|| {
            format!("resolve Cargo workspace directory {}", root_dir.display())
        })?;
        ensure_workspace_directory_windows(root, components)
    }
}

#[cfg(unix)]
fn ensure_workspace_directory_unix(root: PathBuf, components: &[&str]) -> Result<ManagedDirectory> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY);
    let mut handle = options
        .open(&root)
        .into_diagnostic()
        .wrap_err_with(|| format!("open Cargo workspace directory {}", root.display()))?;
    let mut path = root;
    for component in components {
        path.push(component);
        handle = open_or_create_directory_at(&handle, component, &path)?;
    }
    Ok(ManagedDirectory { path, handle })
}

/// Open `component` under `parent`, creating it if it is not there yet.
/// The loop retries the open after a create because a concurrent
/// install may replace the entry in between.
#[cfg(unix)]
fn open_or_create_directory_at(
    parent: &fs::File,
    component: &str,
    path: &Path,
) -> Result<fs::File> {
    loop {
        match open_directory_at(parent, component) {
            Ok(handle) => return Ok(handle),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_directory_at(parent, component)
                    .or_else(accept_existing_directory)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("create Cargo directory {}", path.display()))?;
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotADirectory
                    || error.raw_os_error() == Some(libc::ELOOP) =>
            {
                let path = path.display();
                return Err(miette::miette!(
                    "managed Cargo directory {} must be a real directory",
                    path,
                ));
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect Cargo directory {}", path.display()));
            }
        }
    }
}

fn accept_existing_directory(error: io::Error) -> io::Result<()> {
    match error.kind() {
        // Lost the race with a concurrent install; its directory is as good as ours.
        io::ErrorKind::AlreadyExists => Ok(()),
        _ => Err(error),
    }
}

#[cfg(unix)]
fn open_directory_at(parent: &fs::File, name: &str) -> io::Result<fs::File> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated, `parent` stays open for the call, and
    // the returned descriptor is owned immediately on success.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    file_from_descriptor(descriptor)
}

#[cfg(unix)]
fn create_directory_at(parent: &fs::File, name: &str) -> io::Result<()> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated and `parent` stays open for the call.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o777) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn file_from_descriptor(descriptor: libc::c_int) -> io::Result<fs::File> {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    if descriptor == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful `openat` returns a new descriptor owned by the caller.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        Ok(fs::File::from(descriptor))
    }
}

#[cfg(unix)]
pub(super) fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _, unix::fs::PermissionsExt as _};

    let name = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    // SAFETY: `name` is NUL-terminated, and the directory descriptor remains
    // valid for the call. `O_NOFOLLOW` prevents a file-level symlink redirect.
    let descriptor = unsafe {
        libc::openat(
            directory.handle.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let file = file_from_descriptor(descriptor)?;
    let mode = file.metadata()?.permissions().mode();
    let contents = io::read_to_string(file)?;
    Ok((contents, Some(mode)))
}

#[cfg(windows)]
pub(super) fn read_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(String, Option<u32>)> {
    fs::read_to_string(directory.path.join(name)).map(|contents| (contents, None))
}

#[cfg(unix)]
pub(super) fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    mode: Option<u32>,
) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _, unix::fs::PermissionsExt as _};

    let destination = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    let (temporary, mut file) = create_workspace_temporary(directory, name)?;
    let result = (|| {
        file.write_all(bytes)?;
        if let Some(mode) = mode {
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        file.sync_all()?;
        // SAFETY: both names are valid C strings and both directory descriptors
        // refer to the same live, pinned directory.
        if unsafe {
            libc::renameat(
                directory.handle.as_raw_fd(),
                temporary.as_ptr(),
                directory.handle.as_raw_fd(),
                destination.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })();
    drop(file);
    if result.is_err() {
        // SAFETY: `temporary` is NUL-terminated and the directory handle is valid.
        unsafe {
            libc::unlinkat(directory.handle.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(windows)]
pub(super) fn write_workspace_file(
    directory: &ManagedDirectory,
    name: &str,
    bytes: &[u8],
    _mode: Option<u32>,
) -> io::Result<()> {
    pnpm_fs::write_atomic(&directory.path.join(name), bytes)
}

#[cfg(unix)]
pub(super) fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let wanted = pnpm_fs::relative_path(&directory.path, target);
    let wanted_c = std::ffi::CString::new(wanted.as_os_str().as_bytes())?;
    let name_c = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    let mut warning = None;
    loop {
        // SAFETY: both paths are NUL-terminated and the directory handle is valid.
        if unsafe {
            libc::symlinkat(wanted_c.as_ptr(), directory.handle.as_raw_fd(), name_c.as_ptr())
        } == 0
        {
            return Ok(pnpm_fs::ForceSymlinkOutcome { reused: false, warning });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        match read_link_at(&directory.handle, &name_c) {
            Ok(existing) if existing == wanted => {
                return Ok(pnpm_fs::ForceSymlinkOutcome { reused: true, warning });
            }
            // A symlink pointing somewhere else is ours to replace.
            Ok(_) => unlink_at(directory, &name_c)?,
            // `EINVAL` from `readlinkat` means the entry is not a
            // symlink at all, so it is moved aside rather than removed.
            Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {
                warning = Some(move_occupant_aside(directory, &name_c, name)?);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
}

/// Remove the entry, tolerating a concurrent install having removed it
/// first.
#[cfg(unix)]
fn unlink_at(directory: &ManagedDirectory, name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: `name` is NUL-terminated and the directory handle is valid.
    if unsafe { libc::unlinkat(directory.handle.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        return Ok(());
    }
    Err(error)
}

/// Rename the non-symlink occupying the wanted name out of the way,
/// returning the warning the caller reports for it.
#[cfg(unix)]
fn move_occupant_aside(
    directory: &ManagedDirectory,
    name_c: &std::ffi::CStr,
    name: &str,
) -> io::Result<String> {
    use std::os::fd::AsRawFd as _;

    let ignored_name = format!(
        ".ignored_{name}-{}-{}",
        std::process::id(),
        MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
    );
    let ignored = std::ffi::CString::new(ignored_name.as_bytes())?;
    // SAFETY: both names are NUL-terminated and both descriptors refer to
    // the same valid directory.
    if unsafe {
        libc::renameat(
            directory.handle.as_raw_fd(),
            name_c.as_ptr(),
            directory.handle.as_raw_fd(),
            ignored.as_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(format!(
        "Symlink wanted name was occupied by directory or file. Old entity moved: {:?}{}{} => {ignored_name}",
        directory.path,
        std::path::MAIN_SEPARATOR,
        name,
    ))
}

#[cfg(unix)]
fn read_link_at(directory: &fs::File, name: &std::ffi::CStr) -> io::Result<PathBuf> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStringExt as _};

    let mut capacity = 256;
    loop {
        let mut contents = Vec::<u8>::with_capacity(capacity);
        // SAFETY: the name and directory descriptor are valid, and the buffer has
        // `capacity` writable bytes. `readlinkat` initializes the returned prefix.
        let length = unsafe {
            libc::readlinkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                contents.as_mut_ptr().cast(),
                contents.capacity(),
            )
        };
        if length == -1 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length).expect("readlinkat returned a nonnegative length");
        if length < contents.capacity() {
            // SAFETY: `readlinkat` initialized exactly `length` bytes on success.
            unsafe {
                contents.set_len(length);
            }
            return Ok(std::ffi::OsString::from_vec(contents).into());
        }
        capacity *= 2;
    }
}

#[cfg(windows)]
pub(super) fn force_workspace_symlink(
    directory: &ManagedDirectory,
    target: &Path,
    name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    pnpm_fs::force_symlink_dir(target, &directory.path.join(name))
}

#[cfg(unix)]
fn create_workspace_temporary(
    directory: &ManagedDirectory,
    name: &str,
) -> io::Result<(std::ffi::CString, fs::File)> {
    use std::os::fd::AsRawFd as _;
    loop {
        let temporary_name = format!(
            ".{name}.pnpm-{}-{}",
            std::process::id(),
            MANAGED_TEMP_ID.fetch_add(1, Ordering::Relaxed),
        );
        let temporary = std::ffi::CString::new(temporary_name.as_bytes())?;
        // SAFETY: the name is NUL-terminated, the directory descriptor remains
        // valid, and a successful call returns a new descriptor owned by this function.
        let descriptor = unsafe {
            libc::openat(
                directory.handle.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            )
        };
        match file_from_descriptor(descriptor) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
mod windows;
