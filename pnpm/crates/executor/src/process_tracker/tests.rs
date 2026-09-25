use super::{ProcessTracker, RunningExecution, group_watchdog::GroupWatchdog, spawn_child};
use std::{
    io::{BufRead, BufReader, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::{self, sleep},
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
/// does.
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

#[test]
fn cancellation_leaves_untracked_children_alone() {
    let tracker = ProcessTracker::foreground();
    let mut command = Command::new("sleep");
    command.arg("30");
    let mut tracked = spawn_child(&mut command, Some(&tracker)).expect("spawn tracked child");

    let mut untracked = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn untracked child");

    tracker.cancel();

    assert!(
        !tracked
            .wait()
            .expect("wait for cancelled child")
            .success(),
        "tracked child should be terminated by cancellation",
    );
    assert!(
        !exits_within(&mut untracked, Duration::from_millis(500)),
        "untracked child should remain running after tracker cancellation",
    );

    let _ = untracked.kill();
    let _ = untracked.wait();
}

/// The root shares the test's process group, so only the descendant scan
/// can reach the `sleep` it started. The `sleep` holds the root's stdout,
/// so the pipe reaches EOF only once both are gone.
#[test]
fn cancellation_terminates_descendants_of_tracked_children() {
    let tracker = ProcessTracker::foreground();
    let mut root = Command::new("sh")
        .args(["-c", "sleep 30 & echo ready; wait"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tracked root");
    let registration = tracker.register(RunningExecution::Process {
        pid: root.id(),
        separate_process_group: false,
    });
    let mut stdout = BufReader::new(root.stdout.take().expect("root stdout is piped"));
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read the ready line");
    assert_eq!(ready.trim(), "ready");

    tracker.cancel();
    drop(registration);

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout.read_to_end(&mut Vec::new());
        let _ = sender.send(());
    });
    assert!(
        receiver
            .recv_timeout(Duration::from_secs(10))
            .is_ok(),
        "the tracked root's descendant should have been terminated",
    );
    let _ = root.wait();
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
