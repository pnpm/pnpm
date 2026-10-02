use super::{
    ProcessTracker, RunningExecution, group_is_running, group_watchdog::GroupWatch,
    process_table::is_unsettled, spawn_child,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::{fs::symlink, process::CommandExt},
    process::{self, Child, Command, Stdio},
    ptr,
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

/// Every child in a group of its own is watched by the same watchdog.
#[test]
fn one_watchdog_watches_every_process_group() {
    let tracker = ProcessTracker::default();
    let mut children: Vec<_> = (0..3)
        .map(|_| {
            let mut command = Command::new("sleep");
            command.arg("30");
            spawn_child(&mut command, Some(&tracker)).expect("spawn child")
        })
        .collect();

    let watchdogs = watchdogs();

    tracker.cancel();
    for child in &mut children {
        let _ = child.wait();
    }
    assert_eq!(watchdogs.len(), 1, "one watchdog should watch all three groups");
}

/// Dropping the watch closes the watchdog's pipe the way pnpm's death does.
/// Every group still watched is killed, past one that has exited already,
/// and a released group is left alone.
#[test]
fn a_watch_dropped_kills_every_group_still_watched() {
    let mut watch = GroupWatch::new();
    let mut exited = spawn_group_leader();
    let mut released = spawn_group_leader();
    let mut leaders = [spawn_group_leader(), spawn_group_leader()];
    for leader in [&exited, &released, &leaders[0], &leaders[1]] {
        assert!(watch.watch(leader.id()).expect("watch the group"), "`sh` is available");
    }
    let _ = exited.kill();
    let _ = exited.wait();
    watch.release(released.id());

    drop(watch);

    for leader in &mut leaders {
        assert!(
            exits_within(leader, Duration::from_secs(10)),
            "the group should have been killed once its watchdog lost pnpm",
        );
    }
    assert!(
        !exits_within(&mut released, Duration::from_millis(500)),
        "the released group should still be running",
    );
    let _ = released.kill();
    let _ = released.wait();
}

/// A watchdog that died is replaced on the next watch, and its
/// replacement takes over the groups it was watching.
#[test]
fn a_replaced_watchdog_takes_over_the_groups_of_the_one_that_died() {
    let mut watch = GroupWatch::new();
    let mut leaders = [spawn_group_leader(), spawn_group_leader()];
    assert!(
        watch
            .watch(leaders[0].id())
            .expect("watch the group"),
        "`sh` is available",
    );
    let [dead] = watchdogs()[..] else { panic!("expected one watchdog") };
    // SAFETY: `dead` is this process's own child, the watchdog started
    // above; it is killed and reaped here before its pipe is written again.
    unsafe {
        libc::kill(dead, libc::SIGKILL);
        libc::waitpid(dead, ptr::null_mut(), 0);
    }

    assert!(
        watch
            .watch(leaders[1].id())
            .expect("watch the group"),
        "`sh` is available",
    );
    drop(watch);

    for leader in &mut leaders {
        assert!(
            exits_within(leader, Duration::from_secs(10)),
            "the replacement watchdog should have killed the group",
        );
    }
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
/// shows that nothing in it is still running, also for a command the kernel
/// cut short in the middle of a character.
#[test]
fn a_group_holding_only_zombies_is_not_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[
        (group, b"sleep \xE3\x81", b'Z', group),
        (group + 1, b"node (dev) x", b'Z', group),
        (group + 2, b"sh", b'S', group + 2),
    ]);
    let unreadable = (group + 3).to_string();
    fs::create_dir(table.path().join(unreadable))
        .expect("list a process whose stat cannot be read");
    let only_thread = table
        .path()
        .join(group.to_string())
        .join("task")
        .join(group.to_string());
    fs::create_dir_all(only_thread).expect("list the zombie's own thread");

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(!running, "only zombies are left in the group");
}

/// A command may hold `)` followed by what reads as the fields of a zombie
/// of the group, and, cut short in the middle of a character, bytes that
/// are not UTF-8. Neither hides a live member.
#[test]
fn a_live_member_keeps_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let mut command = format!("node) Z 1 {group} {group} (").into_bytes();
    command.extend_from_slice(b"\xE3\x81");
    let table = process_table(&[(group, &command, b'S', group)]);

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "the group holds a live member");
}

/// A process whose main thread exited reads as a zombie while its other
/// threads keep it running, and only its `task` directory lists them.
#[test]
fn a_member_whose_other_threads_run_keeps_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[(group, b"node", b'Z', group)]);
    let threads = table
        .path()
        .join(group.to_string())
        .join("task");
    for thread in [group, group + 1] {
        fs::create_dir_all(threads.join(thread.to_string())).expect("list a thread");
    }

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "a thread of the member is still running");
}

/// A process whose `stat` cannot be read, as another user's under
/// `hidepid=1`, may still be running, so it counts while the kernel has it
/// in the group, even next to a zombie of the group.
#[test]
fn a_member_whose_stat_cannot_be_read_keeps_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[(group + 1, b"sh", b'Z', group)]);
    let unreadable = group.to_string();
    fs::create_dir(table.path().join(unreadable)).expect("list the leader without its stat");

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "nothing shows that the leader has exited");
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

/// A process that `hidepid=invisible` hides, as another user's, is not
/// listed at all, so a table that shows no member of the group cannot tell
/// such a process from none.
#[test]
fn a_process_table_that_shows_no_member_leaves_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let table = process_table(&[(group + 1, b"sh", b'S', group + 1)]);

    let running = group_is_running(group, Some(table.path()));

    let _ = leader.kill();
    let _ = leader.wait();
    assert!(running, "the kernel still counts the group's leader");
}

/// A member may start another process and exit between the listing and the
/// read of its own entry, and that reading misses the new process, so a
/// second reading that shows other zombies keeps the wait going.
#[test]
fn readings_that_differ_leave_the_group_running() {
    let mut readings = [Some(vec![7]), Some(vec![8, 7])].into_iter();

    let running = is_unsettled(|| readings.next().flatten());

    assert!(running, "the second reading shows a zombie the first one missed");
}

/// Only the table of pnpm's own pid namespace has pnpm as its `self`. An
/// empty directory in place of `/proc` has no `self`, and another
/// namespace's table numbers the processes and their groups differently,
/// so neither can rule a member out.
#[test]
fn a_process_table_of_another_namespace_leaves_the_group_running() {
    let mut leader = spawn_group_leader();
    let group = group_of(&leader);
    let empty = tempfile::tempdir().expect("create a directory");
    let foreign = process_table(&[(group, b"sleep", b'Z', group)]);
    let own = foreign.path().join("self");
    fs::remove_file(&own).expect("unlink the table's `self`");
    symlink((process::id() + 1).to_string(), own).expect("make another process the `self`");

    let running = [empty.path(), foreign.path()].map(|table| group_is_running(group, Some(table)));

    let _ = leader.kill();
    let _ = leader.wait();
    assert_eq!(running, [true, true], "the kernel still counts the group's leader");
}

fn group_of(leader: &Child) -> i32 {
    i32::try_from(leader.id()).expect("the pid fits in a pid_t")
}

/// A process table laid out as Linux lays out `/proc` for this process,
/// its `self`, listing each `(pid, command, state, group)`. A `stat` line
/// is the kernel's: the pid, the command in parentheses, the state, the
/// parent, and the process group.
fn process_table(processes: &[(i32, &[u8], u8, i32)]) -> TempDir {
    let table = tempfile::tempdir().expect("create the process table");
    symlink(process::id().to_string(), table.path().join("self"))
        .expect("make this process the `self`");
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

/// The pids of the watchdogs this process started and has not reaped.
fn watchdogs() -> Vec<i32> {
    let output = Command::new("/bin/ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "args="])
        .output()
        .expect("run ps");
    let own = process::id().to_string();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (pid, parent) = (fields.next()?, fields.next()?);
            let is_watchdog = parent == own && line.contains("trap '' INT TERM HUP");
            is_watchdog.then(|| pid.parse().expect("ps lists numeric pids"))
        })
        .collect()
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
