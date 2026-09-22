use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

/// Create or validate a process-shared lock directory in the current user's
/// temporary area. Unix directories are owner-qualified and forced to mode
/// `0700`, so another local user cannot pre-create or replace their contents.
pub fn secure_temp_lock_dir(name: &str) -> io::Result<PathBuf> {
    secure_lock_dir(std::env::temp_dir(), name)
}

/// Create or validate a process-shared lock directory in a stable location
/// for the current user. Unlike the temporary directory, this location does
/// not vary with per-process temporary-directory settings.
pub fn secure_user_lock_dir(name: &str) -> io::Result<PathBuf> {
    secure_lock_dir(user_lock_root()?, name)
}

fn secure_lock_dir(mut directory: PathBuf, name: &str) -> io::Result<PathBuf> {
    #[cfg(unix)]
    directory.push(format!(
        "{name}-{}",
        // SAFETY: `geteuid` has no preconditions and does not mutate memory.
        unsafe { libc::geteuid() },
    ));
    #[cfg(not(unix))]
    directory.push(name);
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    secure_unix_directory(&directory)?;
    Ok(directory)
}

#[cfg(all(unix, not(target_os = "android")))]
fn user_lock_root() -> io::Result<PathBuf> {
    Ok(PathBuf::from("/tmp"))
}

#[cfg(target_os = "android")]
fn user_lock_root() -> io::Result<PathBuf> {
    Ok(android_user_lock_root(std::env::var_os("HOME")))
}

#[cfg(any(target_os = "android", all(test, unix)))]
fn android_user_lock_root(home: Option<std::ffi::OsString>) -> PathBuf {
    home.filter(|home| !home.is_empty())
        .map_or_else(|| PathBuf::from("/data/local/tmp"), |home| PathBuf::from(home).join(".cache"))
}

#[cfg(windows)]
fn user_lock_root() -> io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::ffi::OsStringExt as _,
        ptr,
        slice,
    };
    use windows_sys::Win32::{
        Foundation::S_OK,
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY,
            SHGetKnownFolderPath,
        },
    };

    let mut path = ptr::null_mut();
    // SAFETY: `path` is a valid out pointer. The returned allocation is
    // inspected through its terminating NUL and released with `CoTaskMemFree`.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY as u32,
            ptr::null_mut(),
            &raw mut path,
        )
    };
    if result != S_OK {
        return Err(io::Error::other(format!(
            "failed to resolve the local application data directory: HRESULT {result:#x}",
        )));
    }
    // SAFETY: a successful call returns a valid NUL-terminated UTF-16 string.
    let len = unsafe {
        let mut len = 0;
        while *path.add(len) != 0 {
            len += 1;
        }
        len
    };
    // SAFETY: `path` points to the NUL-terminated allocation returned above,
    // and `len` excludes the terminator.
    let directory = unsafe { OsString::from_wide(slice::from_raw_parts(path, len)) };
    // SAFETY: the pointer came from `SHGetKnownFolderPath` and has not been freed.
    unsafe { CoTaskMemFree(path.cast()) };
    Ok(PathBuf::from(directory))
}

/// Open a lock file without following symlinks on Unix, then verify that the
/// current user owns the one-link regular file that was opened.
pub fn open_secure_lock_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options
        .create(true)
        .truncate(false)
        .read(true)
        .write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    validate_unix_file(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn secure_unix_directory(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::{
        MetadataExt as _,
        PermissionsExt as _,
    };

    let metadata = fs::symlink_metadata(directory)?;
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    let effective_user = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != effective_user {
        return Err(io::Error::other(format!(
            "lock directory must be a real directory owned by the current user: {}",
            directory.display(),
        )));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn validate_unix_file(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = file.metadata()?;
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    let effective_user = unsafe { libc::geteuid() };
    if !metadata.is_file() || metadata.uid() != effective_user || metadata.nlink() != 1 {
        return Err(io::Error::other("lock must be a regular file owned by the current user"));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
