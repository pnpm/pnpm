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
    /// No handle could be opened, so a caller that needs to signal the
    /// process has to name it by its id and accept what that costs. The
    /// kernel may not pin processes at all, or may have refused this one
    /// for want of a descriptor.
    Unpinnable,
    /// There is no such process, so there is nothing to pin or to signal.
    Gone,
}

/// Whether to wait for the pinned process to exit or only to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    /// Answer now, whether or not the process has exited.
    Poll,
    /// Do not answer until the process has exited.
    UntilExit,
}

/// What becomes of an exit the wait finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaping {
    /// Consume the exit status, so that no later wait finds it.
    Take,
    /// Leave the exit status for whoever owns the process to collect.
    Leave,
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
                // Only "no such process" says the process is gone. Every
                // other refusal leaves it running but unpinned.
                Some(libc::ESRCH) => Pin::Gone,
                _ => Pin::Unpinnable,
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

    /// Whether the pinned process has exited.
    ///
    /// `None` where this kernel cannot wait on a pinned process, which
    /// leaves a caller to ask after the process by its id and accept what
    /// that costs. A process reaped elsewhere, such as under an ignored
    /// `SIGCHLD`, reads as exited.
    #[must_use]
    pub fn exited(&self, waiting: Waiting, reaping: Reaping) -> Option<bool> {
        let Ok(target) = libc::id_t::try_from(self.0.as_raw_fd()) else { return None };
        let flags = libc::WEXITED
            | match waiting {
                Waiting::Poll => libc::WNOHANG,
                Waiting::UntilExit => 0,
            }
            | match reaping {
                Reaping::Take => 0,
                Reaping::Leave => libc::WNOWAIT,
            };
        loop {
            // SAFETY: `siginfo_t` is plain data, and zeroed it reads as no
            // process having changed state, which is what the check below
            // relies on when the wait reports success without filling it.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: `waitid` takes the target and flags by value and
            // writes only the `siginfo_t` behind the pointer, a stack local
            // that outlives the call. `self` keeps the descriptor open
            // across it.
            let waited = unsafe { libc::waitid(libc::P_PIDFD, target, &raw mut info, flags) };
            if waited == 0 {
                // SAFETY: the field is read from a structure the call above
                // either filled or left as this loop zeroed it.
                return Some(unsafe { info.si_pid() } != 0);
            }
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR) => {}
                // No such process to wait for means it is gone.
                Some(libc::ECHILD) => return Some(true),
                _ => return None,
            }
        }
    }

    /// Send `signal` to the pinned process, reporting what came of it.
    ///
    /// A process that exited fails with `ESRCH`, which is not a problem for
    /// any caller: there is nothing left to signal.
    pub fn signal(&self, signal: i32) -> std::io::Result<()> {
        // SAFETY: `pidfd_send_signal` takes the descriptor, the signal, a
        // null `siginfo` asking the kernel to supply one, and no flags. The
        // descriptor is owned by `self` and so open for the call.
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }
}
