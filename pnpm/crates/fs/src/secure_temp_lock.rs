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
    directory.push(format!("{name}-{}", pnpm_ffi_process::effective_user_id()));
    #[cfg(target_os = "wasi")]
    directory.push(format!("{name}-{}", crate::wasi_fs::user_id()));
    #[cfg(not(any(unix, target_os = "wasi")))]
    directory.push(name);
    #[cfg(target_os = "wasi")]
    crate::wasi_fs::secure_directory(&directory)?;
    #[cfg(not(target_os = "wasi"))]
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    secure_owned_directory(&directory)?;
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
    let effective_user = pnpm_ffi_process::effective_user_id();
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
        .map(PathBuf::from)
        .map(|value| value.join(".cache"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp"))
}

#[cfg(windows)]
fn user_lock_root() -> io::Result<PathBuf> {
    pnpm_ffi_fs::local_app_data_directory()
}

/// Open a lock file without following symlinks on Unix, then verify that the
/// current user owns the one-link regular file that was opened.
#[cfg(unix)]
pub fn open_secure_lock_file(path: &Path) -> io::Result<fs::File> {
    let name = path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("expected lock file name: {}", path.display()),
            )
        })?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = open_owned_directory(parent, libc::O_RDONLY)?;
    let file = pnpm_ffi_fs::open_or_create_file_at(
        &directory,
        name,
        pnpm_ffi_fs::FileMode::from_mode(0o600),
    )?;
    validate_unix_file(&file)?;
    Ok(file)
}

#[cfg(not(any(unix, target_os = "wasi")))]
pub fn open_secure_lock_file(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
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
fn secure_owned_directory(directory: &Path) -> io::Result<()> {
    let mode = pnpm_ffi_fs::FileMode::from_mode(0o700);
    match open_owned_directory(directory, libc::O_RDONLY) {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            return crate::file_mode::set_path_handle_mode(
                &open_owned_directory(directory, libc::O_PATH)?,
                mode,
            );
            #[cfg(target_os = "macos")]
            {
                let handle = open_owned_directory(directory, libc::O_SEARCH).map_err(|error| {
                    if error.kind() == io::ErrorKind::PermissionDenied {
                        return io::Error::new(error.kind(), format!(
                            "cannot open lock directory {}: {error}. Restore owner read or search permission on the directory",
                            directory.display(),
                        ));
                    }
                    error
                })?;
                handle.set_permissions(mode.into())
            }
        }
        result => result?.set_permissions(mode.into()),
    }
}

#[cfg(unix)]
fn open_owned_directory(directory: &Path, access: libc::c_int) -> io::Result<fs::File> {
    use std::os::unix::fs::{
        MetadataExt as _,
        OpenOptionsExt as _,
    };

    let handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(access | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(directory)?;
    let metadata = handle.metadata()?;
    let effective_user = pnpm_ffi_process::effective_user_id();
    if metadata.uid() != effective_user {
        return Err(io::Error::other(format!(
            "lock directory must be a real directory owned by the current user: {}",
            directory.display(),
        )));
    }
    Ok(handle)
}

#[cfg(unix)]
fn validate_unix_file(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = file.metadata()?;
    let effective_user = pnpm_ffi_process::effective_user_id();
    if !metadata.is_file() || metadata.uid() != effective_user {
        return Err(io::Error::other("lock must be a regular file owned by the current user"));
    }
    match metadata.nlink() {
        0 => {
            Err(io::Error::new(io::ErrorKind::NotFound, "lock file was unlinked before validation"))
        }
        1 => Ok(()),
        _ => Err(io::Error::other("lock must be a regular file owned by the current user")),
    }
}

#[cfg(all(test, unix))]
mod tests;
