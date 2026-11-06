//! The WASI build's filesystem calls, and the policy pnpm wraps around
//! them.
//!
//! The calls themselves are safe abstractions in
//! [`pnpm_ffi_fs`] and [`pnpm_ffi_webcontainer::fs`]: the first for what
//! WASI's own interface reaches, the second for what only the supervisor
//! can do. What is here is what pnpm decides on top of them, such as which
//! mode a new file inherits and how long a contended lock is waited for.

pub use pnpm_ffi_fs::file_link_count;
pub(crate) use pnpm_ffi_fs::{FileIdentity, open_for_overwrite, path_identity, symlink};
pub use pnpm_ffi_webcontainer::fs::{
    check_file_owner, create_new, executable_access, file_mode, grant_directory_mode_beneath,
    open_directory_nofollow_beneath, open_nofollow, register_sqlite_permissions,
};
pub(crate) use pnpm_ffi_webcontainer::fs::{
    current_umask, path_mode, secure_directory, set_file_mode, user_id,
};

use pnpm_ffi_fs::FileMode;
use std::{io, path::Path};

pub(crate) use pnpm_ffi_fs::file_identity;
pub(crate) use pnpm_ffi_webcontainer::fs::open_lock;

/// Exclusively create a file with private or inherited parent permissions.
pub fn create_inheriting_mode(
    parent: &Path,
    path: &Path,
    requested: Option<FileMode>,
) -> io::Result<std::fs::File> {
    if requested.is_some_and(|mode| mode.mode().trailing_zeros() >= 6) {
        return create_new(path, requested.map_or(0o600, FileMode::mode));
    }
    let parent_mode = match path_mode(parent) {
        Ok(mode) if mode & libc::S_IFMT == libc::S_IFDIR => mode,
        _ => return create_new(path, requested.map_or(0o666, FileMode::mode)),
    };
    let mode = crate::file_mode::inherited_file_mode(
        FileMode::from_mode(parent_mode),
        requested.is_some_and(crate::file_mode::is_executable),
    );
    let file = create_new(path, mode.mode())?;
    grant_file_mode(&file, mode.mode())?;
    Ok(file)
}

fn grant_file_mode(file: &std::fs::File, wanted: u32) -> io::Result<()> {
    let current = file_mode(file)? & 0o777;
    if current | wanted == current {
        return Ok(());
    }
    match set_file_mode(file, current | wanted) {
        Err(error)
            if matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES | libc::EROFS)) =>
        {
            Ok(())
        }
        result => result,
    }
}

/// Try to hold a host process lease until this file is closed.
pub fn try_lock_file(
    file: &std::fs::File,
    mode: crate::LockMode,
) -> Result<(), std::fs::TryLockError> {
    let exclusivity = match mode {
        crate::LockMode::Exclusive => pnpm_ffi_webcontainer::fs::Exclusivity::Exclusive,
        crate::LockMode::Shared => pnpm_ffi_webcontainer::fs::Exclusivity::Shared,
    };
    match pnpm_ffi_webcontainer::fs::try_lock(file, exclusivity) {
        0 => Ok(()),
        libc::EAGAIN => Err(std::fs::TryLockError::WouldBlock),
        errno => Err(std::fs::TryLockError::Error(io::Error::from_raw_os_error(errno))),
    }
}

/// Acquire a host file lease, failing after 30 seconds of contention.
pub fn lock_file(file: &std::fs::File, mode: crate::LockMode) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match try_lock_file(file, mode) {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "WebContainer file lock remained busy for 30 seconds; a nested pnpm command may be waiting on its parent",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}
