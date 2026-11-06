//! Pinning a process so a signal reaches that process or none.

use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};

/// A process held by a handle rather than named by its id.
///
/// An id can be handed to another process once the one that had it exits,
/// so a signal sent to an id found a moment earlier may reach a stranger.
/// While this value lives the kernel keeps the id, so the process it names
/// cannot change, and [`Self::signal`] reaches that process or nothing.
pub struct PinnedTarget(OwnedFd);

/// What came of [`PinnedTarget::pin`].
pub enum Pin {
    /// The process is pinned.
    Target(PinnedTarget),
    /// This kernel cannot pin a process, so a caller that needs to signal
    /// one has to name it by its id and accept what that costs.
    Unsupported,
    /// There is no such process, so there is nothing to pin or to signal.
    Gone,
}

impl PinnedTarget {
    /// Pin the process `pid` names.
    #[must_use]
    pub fn pin(pid: u32) -> Pin {
        let Ok(pid) = i32::try_from(pid) else { return Pin::Gone };
        // SAFETY: `pidfd_open` takes a process id and flags by value and
        // writes no memory. It returns either a new file descriptor that
        // nothing else owns, or a negative error.
        let opened = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if opened < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOSYS) => Pin::Unsupported,
                _ => Pin::Gone,
            };
        }
        let Ok(descriptor) = i32::try_from(opened) else {
            // `pidfd_open` yields an `int`, so this is unreachable. Without
            // a descriptor there is nothing to pin and nothing to close.
            return Pin::Gone;
        };
        // SAFETY: the descriptor is the one the call above returned, which
        // this call owns and nothing else holds.
        Pin::Target(Self(unsafe { OwnedFd::from_raw_fd(descriptor) }))
    }

    /// Send `signal` to the pinned process.
    ///
    /// Best-effort, as [`signal_target`](crate::signal_target) is: a
    /// process that exited is not an error for any caller, because there is
    /// nothing left to signal.
    pub fn signal(&self, signal: i32) {
        // SAFETY: `pidfd_send_signal` takes the descriptor, the signal, a
        // null `siginfo` asking the kernel to supply one, and no flags. The
        // descriptor is owned by `self` and so open for the call.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    }
}
