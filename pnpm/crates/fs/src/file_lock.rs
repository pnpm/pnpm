//! OS advisory locks on a whole file, released when the file is closed.
//! The WASI build takes host leases from `wasi_fs` instead.
//!
//! On Android, Rust 1.97's `File::lock`, `File::lock_shared`, and
//! `File::try_lock` return `Unsupported`, although Android has `flock`.
//! These functions call `flock` there and the standard library elsewhere.

use crate::LockMode;
use std::{
    fs::{File, TryLockError},
    io,
};

/// Block until this handle holds the lock.
#[cfg(not(target_os = "android"))]
pub fn lock_file(file: &File, mode: LockMode) -> io::Result<()> {
    match mode {
        LockMode::Exclusive => file.lock(),
        LockMode::Shared => file.lock_shared(),
    }
}

/// Take the lock if no other handle holds a conflicting one.
#[cfg(not(target_os = "android"))]
pub fn try_lock_file(file: &File, mode: LockMode) -> Result<(), TryLockError> {
    match mode {
        LockMode::Exclusive => file.try_lock(),
        LockMode::Shared => file.try_lock_shared(),
    }
}

#[cfg(target_os = "android")]
pub fn lock_file(file: &File, mode: LockMode) -> io::Result<()> {
    flock(file, flock_operation(mode))
}

#[cfg(target_os = "android")]
pub fn try_lock_file(file: &File, mode: LockMode) -> Result<(), TryLockError> {
    flock(file, flock_operation(mode) | libc::LOCK_NB)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                TryLockError::WouldBlock
            } else {
                TryLockError::Error(error)
            }
        })
}

#[cfg(target_os = "android")]
fn flock_operation(mode: LockMode) -> libc::c_int {
    match mode {
        LockMode::Exclusive => libc::LOCK_EX,
        LockMode::Shared => libc::LOCK_SH,
    }
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
