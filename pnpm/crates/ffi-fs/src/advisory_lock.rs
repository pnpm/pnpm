//! Whole-file advisory locks on Android.
//!
//! Rust 1.97's [`File::lock`](std::fs::File::lock) and its siblings return
//! [`io::ErrorKind::Unsupported`] on Android although the platform has
//! `flock`, so the lock is taken through `flock` directly there.

use std::{fs::File, io, os::fd::AsRawFd as _};

/// Which conflicting locks a request excludes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvisoryLock {
    /// Excludes every other lock on the file.
    Exclusive,
    /// Excludes only exclusive locks, so readers hold it together.
    Shared,
}

/// Take `lock` on `file`, released when every handle to it is closed.
///
/// Fails with [`io::ErrorKind::WouldBlock`] when `blocking` is
/// [`Blocking::No`] and another handle holds a conflicting lock.
pub fn lock_file(file: &File, lock: AdvisoryLock, blocking: Blocking) -> io::Result<()> {
    let operation = match lock {
        AdvisoryLock::Exclusive => libc::LOCK_EX,
        AdvisoryLock::Shared => libc::LOCK_SH,
    } | match blocking {
        Blocking::Yes => 0,
        Blocking::No => libc::LOCK_NB,
    };
    // SAFETY: the `&File` keeps the descriptor open for the whole call, and
    // `flock` takes both arguments by value and writes no memory. The lock
    // it takes is owned by the open file description, not by this call, so
    // there is nothing here to release.
    let locked = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if locked == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Whether a lock request waits for a conflicting holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocking {
    /// Wait until the lock is available.
    Yes,
    /// Report [`io::ErrorKind::WouldBlock`] instead of waiting.
    No,
}
