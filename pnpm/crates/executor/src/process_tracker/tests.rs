use super::{ProcessTracker, group_watchdog::GroupWatchdog, spawn_child};
use std::{
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

/// A foreground tracker keeps a child in the test's own process group
/// while the test holds a terminal, so the terminal's signals reach it.
/// Without a terminal, as on CI, the child gets a group of its own for
/// relayed signals to address. Either way cancellation still ends it.
#[test]
fn foreground_children_share_the_terminal_process_group_only_at_a_terminal() {
    let tracker = ProcessTracker::foreground();
    let mut command = Command::new("sleep");
    command.arg("30");
    let mut child = spawn_child(&mut command, Some(&tracker)).expect("spawn child");
    let child_pid = i32::try_from(child.child_mut().id()).expect("child PID fits i32");

    // SAFETY: both calls only query process-group IDs for this process
    // and the live child spawned immediately above.
    let (parent_group, child_group) = unsafe { (libc::getpgrp(), libc::getpgid(child_pid)) };
    if crate::interrupt::has_controlling_terminal() {
        assert_eq!(child_group, parent_group, "at a terminal the child shares the group");
    } else {
        assert_eq!(child_group, child_pid, "without a terminal the child leads its own group");
    }

    tracker.cancel();
    assert!(
        !child
            .wait()
            .expect("wait for cancelled child")
            .success(),
    );
}

/// Dropping the watchdog unreleased closes its pipe the way pnpm's death
/// does, and the group it watched is killed.
#[test]
fn a_watchdog_dropped_unreleased_kills_the_group() {
    let mut leader = spawn_group_leader();
    let watchdog =
        GroupWatchdog::spawn(leader.id()).expect("spawn the watchdog").expect("`sh` is available");

    drop(watchdog);

    assert!(
        exits_within(&mut leader, Duration::from_secs(10)),
        "the group should have been killed once its watchdog lost pnpm",
    );
}

/// A released watchdog ends without touching the group.
#[test]
fn a_released_watchdog_leaves_the_group_alone() {
    let mut leader = spawn_group_leader();
    let watchdog =
        GroupWatchdog::spawn(leader.id()).expect("spawn the watchdog").expect("`sh` is available");

    watchdog.release();

    assert!(
        !exits_within(&mut leader, Duration::from_millis(500)),
        "the group should still be running after its watchdog was released",
    );
    let _ = leader.kill();
    let _ = leader.wait();
}

/// A `sleep` leading a process group of its own, as a child of
/// [`spawn_child`] does.
fn spawn_group_leader() -> Child {
    let mut command = Command::new("sleep");
    command
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command.spawn().expect("spawn the group leader")
}

fn exits_within(child: &mut Child, deadline: Duration) -> bool {
    let deadline = Instant::now() + deadline;
    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("poll the child")
            .is_some()
        {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}
