//! OS advisory locks on a whole file, released when the file is closed.
//! The WASI build takes host leases from `wasi_fs` instead.
//!
//! On Android, Rust 1.97's `File::lock`, `File::lock_shared`, and
//! `File::try_lock` return `Unsupported`, although Android has `flock`.
//! These functions call `flock` there and the standard library elsewhere.

use std::{
    fs::{File, TryLockError},
    io,
};

/// Block until this handle holds an exclusive or a shared lock on `file`.
#[cfg(not(target_os = "android"))]
pub fn lock_file(file: &File, exclusive: bool) -> io::Result<()> {
    if exclusive { file.lock() } else { file.lock_shared() }
}

/// Take an exclusive or a shared lock on `file` if no other handle holds
/// a conflicting one.
#[cfg(not(target_os = "android"))]
pub fn try_lock_file(file: &File, exclusive: bool) -> Result<(), TryLockError> {
    if exclusive { file.try_lock() } else { file.try_lock_shared() }
}

#[cfg(target_os = "android")]
pub fn lock_file(file: &File, exclusive: bool) -> io::Result<()> {
    flock(file, if exclusive { libc::LOCK_EX } else { libc::LOCK_SH })
}

#[cfg(target_os = "android")]
pub fn try_lock_file(file: &File, exclusive: bool) -> Result<(), TryLockError> {
    let operation = if exclusive { libc::LOCK_EX } else { libc::LOCK_SH };
    flock(file, operation | libc::LOCK_NB)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                TryLockError::WouldBlock
            } else {
                TryLockError::Error(error)
            }
        })
}

#[cfg(target_os = "android")]
fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: `file` owns an open descriptor for the duration of the call.
    if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests;
