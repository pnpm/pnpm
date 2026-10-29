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
    pnpm_ffi_signal::signal_target(kill_target(target), stop.signal());
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
    pnpm_ffi_signal::target_exists(kill_target(target))
}

/// Whether `pid` is in a process group that one of `targets` signals.
pub(super) fn in_signalled_group(pid: i32, targets: &[i32]) -> bool {
    u32::try_from(pid)
        .ok()
        .and_then(pnpm_ffi_signal::process_group_of)
        .is_some_and(|group| group > 0 && targets.contains(&-group))
}

/// Read a signed target the way `kill` reads one: a negative number is the
/// process group that the matching positive number leads.
fn kill_target(target: i32) -> pnpm_ffi_signal::Target {
    if target < 0 {
        pnpm_ffi_signal::Target::Group(target.unsigned_abs())
    } else {
        pnpm_ffi_signal::Target::Process(target.unsigned_abs())
    }
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
    pinned: Option<pnpm_ffi_signal::PinnedTarget>,
}

impl Descendant {
    /// The descendant `pid` that the listing gave `parent`, or `None` once it
    /// is no longer that process.
    fn find(pid: u32, parent: u32) -> Option<Self> {
        let signed = i32::try_from(pid).ok()?;
        #[cfg(target_os = "linux")]
        {
            let pinned = match pnpm_ffi_signal::PinnedTarget::pin(pid) {
                pnpm_ffi_signal::Pin::Target(pinned) => Some(pinned),
                // Nothing to pin on this kernel, so the id is signalled as
                // it is and there is no reuse the check below could catch.
                pnpm_ffi_signal::Pin::Unsupported => None,
                pnpm_ffi_signal::Pin::Gone => return None,
            };
            // With the process pinned its id cannot be reused, so the parent
            // `/proc` gives it now is the one the pinned process has.
            if pinned.is_some() && linux::parent_of(signed)? != parent {
                return None;
            }
            Some(Self { pid: signed, pinned })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = parent;
            Some(Self { pid: signed })
        }
    }

    pub(super) fn pid(&self) -> i32 {
        self.pid
    }

    pub(super) fn signal(&self, stop: Stop) {
        #[cfg(target_os = "linux")]
        if let Some(pinned) = &self.pinned {
            pinned.signal(stop.signal());
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

/// Reading a process's parent from `/proc`.
#[cfg(target_os = "linux")]
mod linux {
    /// The parent pid `/proc` gives `pid`.
    pub(super) fn parent_of(pid: i32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, fields) = stat.rsplit_once(") ")?;
        fields
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
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
