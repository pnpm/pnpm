//! Signalling a Unix task, and finding the descendants a signal to its
//! process group does not reach.

use super::{RunningExecution, Stop, spawn};
use crate::process::Command;
use std::{collections::HashMap, io::Read, process::Stdio, time::Duration};

impl Stop {
    fn signal(self) -> libc::c_int {
        match self {
            Self::Graceful => libc::SIGTERM,
            Self::Forced => libc::SIGKILL,
        }
    }
}

impl RunningExecution {
    /// What a signal for this execution addresses: the process group the
    /// child leads, or the child alone.
    pub(super) fn signal_target(&self) -> Option<i32> {
        match self {
            Self::Process { pid, separate_process_group } => {
                let pid = i32::try_from(*pid).ok()?;
                Some(if *separate_process_group { -pid } else { pid })
            }
            Self::Group { leader } => i32::try_from(*leader).ok().map(|leader| -leader),
            Self::Emulated(_) => None,
        }
    }
}

pub(super) fn terminate_process(pid: u32, separate_process_group: bool, stop: Stop) {
    let execution = RunningExecution::Process { pid, separate_process_group };
    if let Some(target) = execution.signal_target() {
        signal(target, stop);
    }
}

/// Signal `target`: a negative target signals the process group
/// [`super::spawn_child`] created, a positive one a child or a descendant
/// pnpm listed.
pub(super) fn signal(target: i32, stop: Stop) {
    // SAFETY: plain `kill`; ESRCH is harmless when the target exited
    // concurrently.
    unsafe {
        libc::kill(target, stop.signal());
    }
}

/// Signal the group a reaped child led, while it still has a member.
pub(super) fn signal_group(leader: u32, stop: Stop) {
    if let Ok(leader) = i32::try_from(leader)
        && group_has_member(-leader)
    {
        signal(-leader, stop);
    }
}

/// Whether the process group `target` names (negative) still has a member.
/// While it does, its id cannot be handed to another process or group, so
/// a signal to it reaches only the group pnpm created.
pub(super) fn group_has_member(target: i32) -> bool {
    // SAFETY: signal 0 only probes.
    unsafe { libc::kill(target, 0) == 0 }
}

/// Whether `pid` is in a process group that one of `targets` signals.
pub(super) fn in_signalled_group(pid: i32, targets: &[i32]) -> bool {
    // SAFETY: `getpgid` only reads the process table, and fails with -1 for a
    // process that has exited.
    let group = unsafe { libc::getpgid(pid) };
    group > 0 && targets.contains(&-group)
}

/// A descendant of a task, found through the parent links of a process
/// listing.
///
/// Its pid may be reused by another process between the listing and a
/// signal, so on Linux the descendant is pinned through a pidfd as soon as it
/// is found, and kept only if it still has the parent the listing named. A
/// pidfd keeps its pid from being reused, and a signal sent through it
/// reaches that process or none. Other Unix systems have no such handle, and
/// signal the pid right after the listing.
pub(super) struct Descendant {
    pid: i32,
    #[cfg(target_os = "linux")]
    pidfd: Option<std::os::fd::OwnedFd>,
}

impl Descendant {
    /// The descendant `pid` that the listing gave `parent`, or `None` once it
    /// is no longer that process.
    fn find(pid: u32, parent: u32) -> Option<Self> {
        let pid = i32::try_from(pid).ok()?;
        #[cfg(target_os = "linux")]
        {
            let pidfd = linux::pin(pid, parent)?;
            Some(Self { pid, pidfd })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = parent;
            Some(Self { pid })
        }
    }

    pub(super) fn pid(&self) -> i32 {
        self.pid
    }

    pub(super) fn signal(&self, stop: Stop) {
        #[cfg(target_os = "linux")]
        if let Some(pidfd) = &self.pidfd {
            linux::signal(pidfd, stop.signal());
            return;
        }
        signal(self.pid, stop);
    }
}

pub(super) fn descendant_processes(roots: &[u32]) -> Vec<Descendant> {
    if roots.is_empty() {
        return Vec::new();
    }
    let Some(listing) = process_listing() else {
        return Vec::new();
    };
    let children = parse_parent_child_pids(&listing);
    let mut descendants = Vec::new();
    let mut stack = roots.to_vec();
    while let Some(parent) = stack.pop() {
        let found = children
            .get(&parent)
            .into_iter()
            .flatten();
        for &pid in found {
            stack.push(pid);
            descendants.extend(Descendant::find(pid, parent));
        }
    }
    descendants
}

/// Pinning a process and signalling it through a pidfd.
#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    /// A pidfd for `pid` while it is still the child of `parent`. `Some(None)`
    /// when the kernel has no pidfds, which leaves the pid to be signalled as
    /// it is; `None` when `pid` is gone or is another process now.
    pub(super) fn pin(pid: i32, parent: u32) -> Option<Option<OwnedFd>> {
        // SAFETY: `pidfd_open` takes a pid and flags and returns a new file
        // descriptor, which `OwnedFd` then owns alone.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            let unsupported = std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOSYS);
            return unsupported.then_some(None);
        }
        // SAFETY: see above; `fd` is a valid descriptor nothing else owns.
        let pidfd = unsafe { OwnedFd::from_raw_fd(i32::try_from(fd).ok()?) };
        // With the pidfd open the pid cannot be reused, so the process
        // `/proc` describes now is the one the pidfd holds.
        (parent_of(pid)? == parent).then_some(Some(pidfd))
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

    pub(super) fn signal(pidfd: &OwnedFd, signal: libc::c_int) {
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
            );
        }
    }
}

/// Run `ps` and read its whole listing, giving up on anything that does not
/// finish promptly. A `ps` that hangs is killed and its output discarded: a
/// partial listing would name the wrong parents.
fn process_listing() -> Option<String> {
    let mut command = Command::new("/bin/ps");
    command
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stdout(Stdio::piped());
    let mut child = spawn(&mut command).ok()?;
    let mut stdout = child.stdout.take()?;
    let output = std::thread::spawn(move || {
        let mut listing = String::new();
        stdout.read_to_string(&mut listing).map(|_| listing)
    });
    let completed = wait_briefly(&mut child);
    if !completed {
        let _ = child.kill();
        let _ = child.wait();
    }
    let listing = output.join().ok()?.ok()?;
    completed.then_some(listing)
}

/// Poll a child for up to half a second, reporting whether it exited.
fn wait_briefly(child: &mut crate::process::Child) -> bool {
    for _ in 0..50 {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return false,
        }
    }
    false
}

/// The child pids of every parent named in a `pid ppid` listing.
fn parse_parent_child_pids(listing: &str) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (Ok(pid), Ok(parent)) = (pid.parse(), parent.parse()) else {
            continue;
        };
        children
            .entry(parent)
            .or_default()
            .push(pid);
    }
    children
}
