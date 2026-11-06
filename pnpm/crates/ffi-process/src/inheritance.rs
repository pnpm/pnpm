//! Whether a descriptor is passed on across an `exec`.

use std::os::fd::{AsRawFd as _, BorrowedFd};

/// Let `descriptor` be inherited across an `exec`, by clearing its
/// close-on-exec flag.
///
/// Reports whether the flag could be cleared. For a test that has to
/// recreate the inheritance the standard library's own two-step pipe setup
/// leaves open on macOS; nothing in pnpm wants this in earnest.
#[must_use]
pub fn allow_inheritance(descriptor: BorrowedFd<'_>) -> bool {
    // SAFETY: the borrow keeps the descriptor open for both calls, and
    // `fcntl` takes its arguments by value and writes no memory for either
    // of these commands.
    unsafe {
        let flags = libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD);
        flags >= 0
            && libc::fcntl(descriptor.as_raw_fd(), libc::F_SETFD, flags & !libc::FD_CLOEXEC) == 0
    }
}
