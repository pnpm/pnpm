use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Create or validate a process-shared lock directory in the current user's
/// temporary area. Unix directories are owner-qualified and forced to mode
/// `0700`, so another local user cannot pre-create or replace their contents.
pub fn secure_temp_lock_dir(name: &str) -> io::Result<PathBuf> {
    secure_lock_dir(crate::temp_dir(), name)
}

/// Create or validate a process-shared lock directory in a stable location
/// for the current user. Unlike the temporary directory, this location does
/// not vary with per-process temporary-directory settings.
///
/// On Unix the location is `$XDG_RUNTIME_DIR` when it holds an absolute
/// path to a directory the current user owns, the per-user runtime directory
/// the XDG Base Directory spec defines for locks, and `/tmp` otherwise.
/// Processes coordinate only while they agree on it.
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
    #[cfg(target_os = "wasi")]
    directory.push(format!("{name}-{}", crate::wasi_fs::user_id()));
    #[cfg(not(any(unix, target_os = "wasi")))]
    directory.push(name);
    #[cfg(target_os = "wasi")]
    crate::wasi_fs::secure_directory(&directory)?;
    #[cfg(not(target_os = "wasi"))]
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    secure_unix_directory(&directory)?;
    Ok(directory)
}

#[cfg(target_os = "wasi")]
fn user_lock_root() -> io::Result<PathBuf> {
    Ok(PathBuf::from("/tmp"))
}

#[cfg(all(unix, not(target_os = "android")))]
fn user_lock_root() -> io::Result<PathBuf> {
    Ok(xdg_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"))
        .unwrap_or_else(|| PathBuf::from("/tmp")))
}

#[cfg(target_os = "android")]
fn user_lock_root() -> io::Result<PathBuf> {
    Ok(xdg_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"))
        .unwrap_or_else(|| android_user_lock_root(std::env::var_os("HOME"))))
}

/// The XDG Base Directory spec tells applications to ignore a relative
/// `XDG_RUNTIME_DIR`. One owned by another user is inherited through `su`
/// or `sudo --preserve-env`, and this user cannot create locks in it. One
/// that other users can write to lets them rename a held lock directory
/// away, or pre-create one this user then refuses. One its owner cannot
/// write to or search cannot hold the lock directory.
#[cfg(unix)]
fn xdg_runtime_dir(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt as _;

    const OWNER_WRITE_AND_SEARCH: u32 = 0o300;
    const GROUP_OR_OTHER_WRITE: u32 = 0o022;
    let path = PathBuf::from(value?);
    // SAFETY: `geteuid` has no preconditions and does not mutate memory.
    let effective_user = unsafe { libc::geteuid() };
    let usable = fs::metadata(&path)
        .is_ok_and(|metadata| {
            let mode = metadata.mode();
            metadata.is_dir()
                && metadata.uid() == effective_user
                && mode & OWNER_WRITE_AND_SEARCH == OWNER_WRITE_AND_SEARCH
                && mode & GROUP_OR_OTHER_WRITE == 0
        });
    (path.is_absolute() && usable).then_some(path)
}

#[cfg(any(target_os = "android", all(test, unix)))]
fn android_user_lock_root(home: Option<std::ffi::OsString>) -> PathBuf {
    home.filter(|home| !home.is_empty())
        .map_or_else(|| PathBuf::from("/data/local/tmp"), |home| PathBuf::from(home).join(".cache"))
}

#[cfg(windows)]
fn user_lock_root() -> io::Result<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, ptr, slice};
    use windows_sys::Win32::{
        Foundation::S_OK,
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
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
#[cfg(not(target_os = "wasi"))]
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

#[cfg(target_os = "wasi")]
pub fn open_secure_lock_file(path: &Path) -> io::Result<fs::File> {
    let file = match crate::wasi_fs::create_new(path, 0o600) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            crate::wasi_fs::open_lock(path)?
        }
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(format!("lock must be a regular file: {}", path.display())));
    }
    Ok(file)
}

/// The lock file standing for `path` in the per-user lock directory
/// `namespace`: `<directory>/<hash of path>.<extension>`.
///
/// Resolve `path` first, so that every alias of one location shares the
/// file.
pub fn secure_user_lock_file_path(
    namespace: &str,
    path: &Path,
    extension: &str,
) -> io::Result<PathBuf> {
    let key = pnpm_crypto_hash::create_hex_hash_bytes(&native_path_bytes(path));
    Ok(secure_user_lock_dir(namespace)?.join(format!("{key}.{extension}")))
}

#[cfg(unix)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .as_encoded_bytes()
        .to_vec()
}

#[cfg(unix)]
fn secure_unix_directory(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

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
