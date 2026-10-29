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

/// Whether `pid` is in a process group that one of `targets` signals.
pub(super) fn in_signalled_group(pid: i32, targets: &[i32]) -> bool {
    // SAFETY: `getpgid` only reads the process table, and fails with -1 for a
    // process that has exited.
    let group = unsafe { libc::getpgid(pid) };
    group > 0 && targets.contains(&-group)
}

pub(super) fn descendant_processes(roots: &[u32]) -> Vec<i32> {
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
        for &pid in children
            .get(&parent)
            .into_iter()
            .flatten()
        {
            stack.push(pid);
            if let Ok(pid) = i32::try_from(pid) {
                descendants.push(pid);
            }
        }
    }
    descendants
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
