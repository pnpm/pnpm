use super::{
    ProcessTracker, RunningExecution, group_is_running, group_watchdog::GroupWatchdog, spawn_child,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::{self, sleep},
    time::{Duration, Instant},
};
use tempfile::TempDir;

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

/// The kernel counts a zombie among its group's members until the zombie is
/// reaped, so the group stays real for the probe. Only the process table
/// shows that nothing in it is still running.
#[test]
fn a_group_holding_only_zombies_is_not_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[
        (group, b"sleep", b'Z', group),
        (group + 1, b"node (dev) x", b'Z', group),
        (group + 2, b"sh", b'S', group + 2),
    ]);
    let unreadable = (group + 3).to_string();
    fs::create_dir(table.path().join(unreadable))
        .expect("list a process whose stat cannot be read");

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(!running, "only zombies are left in the group");
}

/// A command may hold `)` and, cut short in the middle of a character,
/// bytes that are not UTF-8. Neither hides a live member.
#[test]
fn a_live_member_keeps_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[(group, b"node) Z 1 0 0 (\xE3\x81", b'S', group)]);

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "the group holds a live member");
}

/// A table that cannot be listed cannot tell a zombie from a live member,
/// so it leaves the answer to the kernel's count.
#[test]
fn a_process_table_that_cannot_be_listed_leaves_the_group_running() {
    let mut leader = spawn_group_leader();
    let root = tempfile::tempdir().expect("create a directory");

    let running = group_is_running(group_of(&leader), Some(&root.path().join("proc")));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "the kernel still counts the group's leader");
}

fn group_of(leader: &Child) -> i32 {
    i32::try_from(leader.id()).expect("the pid fits in a pid_t")
}

/// A process table laid out as Linux lays out `/proc`, listing each
/// `(pid, command, state, group)`. A `stat` line is the kernel's: the pid,
/// the command in parentheses, the state, the parent, and the process
/// group.
fn process_table(processes: &[(i32, &[u8], u8, i32)]) -> TempDir {
    let table = tempfile::tempdir().expect("create the process table");
    for &(pid, command, state, group) in processes {
        let entry = table.path().join(pid.to_string());
        fs::create_dir(&entry).expect("create a process entry");
        let mut stat = format!("{pid} (").into_bytes();
        stat.extend_from_slice(command);
        stat.extend_from_slice(format!(") {} 1 {group} {group}\n", char::from(state)).as_bytes());
        fs::write(entry.join("stat"), stat).expect("write the process's stat");
    }
    table
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
