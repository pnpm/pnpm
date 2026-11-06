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
    pnpm_ffi_fs::lock_file(file, advisory_lock(mode), pnpm_ffi_fs::Blocking::Yes)
}

#[cfg(target_os = "android")]
pub fn try_lock_file(file: &File, mode: LockMode) -> Result<(), TryLockError> {
    pnpm_ffi_fs::lock_file(file, advisory_lock(mode), pnpm_ffi_fs::Blocking::No)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                TryLockError::WouldBlock
            } else {
                TryLockError::Error(error)
            }
        })
}

#[cfg(target_os = "android")]
fn advisory_lock(mode: LockMode) -> pnpm_ffi_fs::AdvisoryLock {
    match mode {
        LockMode::Exclusive => pnpm_ffi_fs::AdvisoryLock::Exclusive,
        LockMode::Shared => pnpm_ffi_fs::AdvisoryLock::Shared,
    }
}

#[cfg(test)]
mod tests;
