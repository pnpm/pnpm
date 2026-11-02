//! Sending a signal to a child or to a child's process group.

/// What a signal is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One process, by its id.
    Process(u32),
    /// Every process in the group a child leads, by that child's id.
    Group(u32),
}

/// Send `signal` to `target`.
///
/// Best-effort: a target that exited concurrently is not an error for any
/// caller, because there is nothing left to signal.
///
/// Async-signal-safe.
pub fn signal_target(target: Target, signal: i32) {
    let Some(target) = raw_target(target) else { return };
    // SAFETY: `kill` takes both arguments by value and writes no memory. A
    // target that no longer exists is reported with `ESRCH` rather than
    // reaching an unrelated process, because the caller passes the id of a
    // child it has not yet reaped, whose id the kernel therefore cannot
    // have reused.
    unsafe {
        libc::kill(target, signal);
    }
}

/// Whether `target` still exists.
///
/// A target the caller may not signal exists all the same: the kernel
/// reports `EPERM` for it rather than `ESRCH`, and reading that as absent
/// would make a probe answer "gone" for a process that is running.
///
/// Async-signal-safe.
#[must_use]
pub fn target_exists(target: Target) -> bool {
    let Some(target) = raw_target(target) else { return false };
    // SAFETY: `kill` with signal `0` only probes for the target's
    // existence, delivering nothing. It takes both arguments by value and
    // writes no memory.
    unsafe {
        libc::kill(target, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// Whether the group led by `leader` still holds a process that has not
/// exited.
///
/// Children of this process that have exited are reaped first, so a zombie
/// pnpm itself could reap does not keep the group looking alive. A zombie
/// another process adopted does, since only its parent can reap it.
///
/// Async-signal-safe.
#[must_use]
pub fn group_has_members(leader: u32) -> bool {
    let Some(group) = raw_target(Target::Group(leader)) else { return false };
    // SAFETY: `waitpid` with `WNOHANG` never blocks, and a null status
    // pointer is the documented way to discard the exit status. `kill` with
    // signal `0` only probes for the target's existence. Both take their
    // arguments by value and write no memory.
    unsafe {
        while libc::waitpid(group, std::ptr::null_mut(), libc::WNOHANG) > 0 {}
        libc::kill(group, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

/// Wait for the child `pid` names to exit, and reap it.
///
/// Reports whether it was reaped. Only a child of the calling process can
/// be: anything else reads as not reaped.
#[must_use]
pub fn reap(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    // SAFETY: a null status pointer is the documented way to discard the
    // exit status, and the other two arguments are taken by value. The call
    // writes no memory.
    unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) == pid }
}

/// The process group `pid` belongs to, or `None` once no such process
/// exists.
#[must_use]
pub fn process_group_of(pid: u32) -> Option<i32> {
    let pid = i32::try_from(pid).ok()?;
    // SAFETY: `getpgid` takes the id by value and writes no memory. It
    // reports `-1` rather than a group for a process that does not exist.
    let group = unsafe { libc::getpgid(pid) };
    (group >= 0).then_some(group)
}

/// Whether this process's group is the foreground group of its controlling
/// terminal: the group a `Ctrl+C` there interrupts as a whole.
///
/// Async-signal-safe: `open`, `tcgetpgrp`, `getpgrp` and `close` all are.
#[must_use]
pub fn holds_the_terminal() -> bool {
    let Some(terminal) = ControllingTerminal::open() else { return false };
    // SAFETY: `terminal.0` is the descriptor the open returned and is still
    // open, since only the guard's drop closes it. `tcgetpgrp` and
    // `getpgrp` write no memory and take their arguments by value.
    unsafe { libc::tcgetpgrp(terminal.0) == libc::getpgrp() }
}

/// Whether this process has a controlling terminal.
///
/// Without one, only `kill` can signal pnpm, and nothing signals its
/// children but pnpm.
///
/// Async-signal-safe.
#[must_use]
pub fn has_controlling_terminal() -> bool {
    ControllingTerminal::open().is_some()
}

/// An open descriptor on `/dev/tty`, closed when dropped.
///
/// Opening it succeeds only for a process that has a controlling terminal,
/// which is what both queries above rest on.
struct ControllingTerminal(libc::c_int);

impl ControllingTerminal {
    fn open() -> Option<Self> {
        // SAFETY: the path is a NUL-terminated literal borrowed for the
        // call. The flags create nothing, so `open` reads no variadic mode
        // argument and none is passed.
        let descriptor = unsafe {
            libc::open(c"/dev/tty".as_ptr(), libc::O_RDONLY | libc::O_NOCTTY | libc::O_CLOEXEC)
        };
        (descriptor >= 0).then_some(Self(descriptor))
    }
}

impl Drop for ControllingTerminal {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the descriptor the open returned, owned by
        // this guard, and a guard is dropped once, so the descriptor is
        // closed exactly once.
        unsafe {
            libc::close(self.0);
        }
    }
}

fn raw_target(target: Target) -> Option<libc::pid_t> {
    match target {
        Target::Process(pid) => i32::try_from(pid).ok(),
        Target::Group(leader) => i32::try_from(leader).ok().map(|leader| -leader),
    }
}

#[cfg(test)]
mod tests;
