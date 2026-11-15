//! Pinning a process and signalling and reaping it through a pidfd.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// A pidfd for `pid` while it is still the child of `parent`. `Some(None)`
/// when the kernel has no pidfds, which leaves the pid to be signalled as
/// it is; `None` when `pid` is gone or is another process now.
pub(super) fn pin(pid: i32, parent: u32) -> Option<Option<OwnedFd>> {
    let pidfd = match open(pid) {
        Ok(pidfd) => pidfd,
        Err(error) => return (error.raw_os_error() == Some(libc::ENOSYS)).then_some(None),
    };
    // With the pidfd open the pid cannot be reused, so the process
    // `/proc` describes now is the one the pidfd holds.
    (parent_of(pid)? == parent).then_some(Some(pidfd))
}

/// A pidfd for the process `pid` names now.
pub(in crate::process_tracker) fn open(pid: i32) -> std::io::Result<OwnedFd> {
    // SAFETY: `pidfd_open` takes a pid and flags and returns a new file
    // descriptor, which `OwnedFd` then owns alone.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = i32::try_from(fd).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    // SAFETY: see above; `fd` is a valid descriptor nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Whether the child `pidfd` holds has exited, reaping it if so. `flags`
/// adds `WNOHANG` or `WNOWAIT` to `WEXITED`. A child reaped elsewhere,
/// such as under an ignored `SIGCHLD`, has exited. `None` when the
/// kernel cannot wait on a pidfd.
pub(in crate::process_tracker) fn exited(pidfd: &OwnedFd, flags: libc::c_int) -> Option<bool> {
    let id = libc::id_t::try_from(pidfd.as_raw_fd()).ok()?;
    loop {
        // SAFETY: `siginfo_t` is plain data, and zeroed it reads as no
        // child having changed state.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `waitid` writes only to `info`, which outlives the call.
        let waited =
            unsafe { libc::waitid(libc::P_PIDFD, id, &raw mut info, libc::WEXITED | flags) };
        if waited == 0 {
            // SAFETY: `waitid` filled `info` for a child, or left it zeroed.
            return Some(unsafe { info.si_pid() } != 0);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => {}
            Some(libc::ECHILD) => return Some(true),
            _ => return None,
        }
    }
}

/// The parent pid `/proc` gives `pid`.
fn parent_of(pid: i32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Whether the signal was sent.
pub(in crate::process_tracker) fn signal(pidfd: &OwnedFd, signal: libc::c_int) -> bool {
    // SAFETY: `pidfd_send_signal` takes the pidfd, the signal, no
    // `siginfo`, and no flags. It fails harmlessly once the process
    // exited.
    unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        ) == 0
    }
}
