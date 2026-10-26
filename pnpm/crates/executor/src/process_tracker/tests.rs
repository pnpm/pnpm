use super::{
    ProcessTracker, RunningExecution, group_is_running, group_watchdog::GroupWatch,
    process_table::is_unsettled, spawn_child,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::{fs::symlink, process::CommandExt},
    path::Path,
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

/// A signal pnpm takes while a child is being spawned reaches no child:
/// the handler passes it on to the children that have joined the relay,
/// and this one has not. Nothing else would end that child, so pnpm would
/// wait for it for as long as it ran. pnpm outliving the signal is the
/// other half of the same contract: a handler that finds nothing to wait
/// for ends pnpm, which here would end the test.
#[test]
fn a_signal_taken_while_a_child_is_spawned_reaches_it_once_it_joins() {
    a_signal_taken_while_a_child_is_spawned(Raise::OnceTheChildTraps);
}

/// A signal can also arrive before the child exists at all, which is most
/// of the gap: the spawn itself, and the watchdog's first `sh` behind it.
/// The claim covers that too, and the child it creates is still given the
/// signal rather than being left running by the spawn that made it.
#[test]
fn a_signal_taken_before_a_child_exists_reaches_it_once_it_joins() {
    a_signal_taken_while_a_child_is_spawned(Raise::BeforeTheSpawn);
}

/// When the test raises the signal, relative to the child's creation.
#[derive(Clone, Copy)]
enum Raise {
    BeforeTheSpawn,
    OnceTheChildTraps,
}

fn a_signal_taken_while_a_child_is_spawned(raise: Raise) {
    let directory = TempDir::new().expect("create a temporary directory");
    let trapped = directory.path().join("trapped");
    // Claimed before the spawn, as `spawn_child` claims it, which is also
    // what arms the handler the signal below needs.
    let mut spawning = crate::interrupt::spawning_child();
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!(
            "trap 'exit 42' INT; : > '{trapped}.tmp'; mv '{trapped}.tmp' '{trapped}'; while :; do sleep 0.05; done",
            trapped = trapped.display(),
        ))
        // A child that leads a process group of its own is the one a
        // terminal's signals never reach, and the one a parallel run
        // spawns.
        .process_group(0);
    if matches!(raise, Raise::BeforeTheSpawn) {
        interrupt_this_process();
    }
    let mut child = command.spawn().expect("spawn the child");
    spawning.spawned();
    // The marker says the child traps the signal, so the child is ready
    // for it and only the relay is not.
    wait_for_marker(&mut child, &trapped);

    if matches!(raise, Raise::OnceTheChildTraps) {
        interrupt_this_process();
    }
    let relay = crate::interrupt::relay_to_child(child.id(), true);
    spawning.joined(&relay);

    if !exits_within(&mut child, Duration::from_secs(30)) {
        kill_group(&mut child);
        panic!("the child must be given the signal once it joins the relay");
    }
    let status = child.wait().expect("wait for the child");
    dbg!(status);
    assert_eq!(status.code(), Some(42), "the child must end through the signal it traps");
}

/// A child that misses two interrupts owes the escalation as well as the
/// interrupt: the handler gives a registered child `SIGTERM` on the
/// second, and a child that joined late must not be gentler for having
/// been spawned while they arrived. The gap spans the spawn and the
/// watchdog behind it, so two presses of `Ctrl+C` fit inside it.
#[test]
fn two_signals_taken_while_a_child_is_spawned_both_reach_it() {
    let directory = TempDir::new().expect("create a temporary directory");
    let (ready, received) = (directory.path().join("ready"), directory.path().join("received"));
    let mut spawning = crate::interrupt::spawning_child();
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!(
            "trap 'echo INT >> {received}' INT; trap 'echo TERM >> {received}' TERM; \
             : > {ready}; while :; do sleep 0.05; done",
            received = received.display(),
            ready = ready.display(),
        ))
        .process_group(0);
    let mut child = command.spawn().expect("spawn the child");
    spawning.spawned();
    // Recording rather than exiting, so the second signal has something
    // to arrive at and neither races the child's exit.
    wait_for_marker(&mut child, &ready);

    interrupt_this_process();
    interrupt_this_process();
    let relay = crate::interrupt::relay_to_child(child.id(), true);
    spawning.joined(&relay);

    let both = ["INT", "TERM"];
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = String::new();
    while Instant::now() < deadline {
        seen = std::fs::read_to_string(&received).unwrap_or_default();
        if both
            .iter()
            .all(|signal| seen.contains(signal))
        {
            break;
        }
        sleep(Duration::from_millis(20));
    }
    kill_group(&mut child);
    for signal in both {
        assert!(seen.contains(signal), "the child missed {signal}, having received {seen:?}");
    }
}

/// Raise `SIGINT` on this process, which the claim in the caller has
/// armed a handler for.
///
/// It is this process's signal alone because nextest gives every test a
/// process of its own; under a runner that shares one it would reach
/// another test's children.
fn interrupt_this_process() {
    // SAFETY: a signal to this process, whose handler is installed.
    assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0, "raise the signal");
}

/// Block until the child creates `marker`, the file a fixture writes once
/// it is ready for a signal.
///
/// A child that exits without writing it fails the test there, rather than
/// leaving the test polling for a marker nothing will write.
fn wait_for_marker(child: &mut Child, marker: &Path) {
    while !marker.exists() {
        if child
            .try_wait()
            .expect("poll the child")
            .is_some()
        {
            // A child that wrote the marker and exited in the time since
            // the loop last looked was ready, not broken.
            assert!(marker.exists(), "the child exited before it was ready for a signal");
            return;
        }
        sleep(Duration::from_millis(5));
    }
}

/// Kill the group the child leads and reap it, so a failing test leaves no
/// fixture looping behind it.
fn kill_group(child: &mut Child) {
    // SAFETY: the group of a child this test spawned, which leads a group
    // of its own.
    unsafe {
        libc::kill(-i32::try_from(child.id()).expect("pid fits i32"), libc::SIGKILL);
    }
    let _ = child.wait();
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
