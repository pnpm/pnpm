use super::{ProcessTracker, spawn_child};
use std::process::Command;

#[test]
fn foreground_children_stay_in_the_terminal_process_group() {
    let tracker = ProcessTracker::foreground();
    let mut command = Command::new("sleep");
    command.arg("30");
    let mut child = spawn_child(&mut command, Some(&tracker)).expect("spawn child");
    let child_pid = i32::try_from(child.child_mut().id()).expect("child PID fits i32");

    // SAFETY: both calls only query process-group IDs for this process
    // and the live child spawned immediately above.
    let (parent_group, child_group) = unsafe { (libc::getpgrp(), libc::getpgid(child_pid)) };
    assert_eq!(child_group, parent_group);

    tracker.cancel();
    assert!(!child.wait().expect("wait for cancelled child").success());
}
