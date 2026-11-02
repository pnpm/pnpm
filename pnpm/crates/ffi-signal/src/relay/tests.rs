use super::{Target, process_group_of, target_exists};

#[test]
fn a_process_group_is_readable_for_this_process() {
    let own = std::process::id();
    assert!(process_group_of(own).is_some_and(|group| group > 0));
}

/// Pins that a target that does not exist reads as absent rather than as
/// an error the caller has to interpret. `u32::MAX` is not an id the kernel
/// assigns.
#[test]
fn an_absent_target_does_not_exist() {
    assert!(!target_exists(Target::Process(u32::MAX)));
    assert!(process_group_of(u32::MAX).is_none());
}

/// A reaped child, whose id fits `i32` and names no process. `u32::MAX`
/// does not reach the lookup at all: it fails the conversion first, which
/// leaves what `getpgid` reports for an absent process untested.
#[test]
fn a_reaped_pid_has_no_process_group() {
    let mut child = std::process::Command::new("true").spawn().expect("spawn a child to reap");
    let pid = child.id();
    child.wait().expect("reap the child");

    assert!(process_group_of(pid).is_none(), "pid {pid} was reaped");
    assert!(!target_exists(Target::Process(pid)));
}

/// The other half of the same branch: this process exists, so the probe
/// cannot be reporting absence for everything.
#[test]
fn a_live_target_exists() {
    assert!(target_exists(Target::Process(std::process::id())));
}

/// Pins that a process the caller may not signal reads as present. PID 1
/// is owned by root, so an unprivileged test cannot signal it; where the
/// test does run as root it can, and either way the answer is that PID 1
/// exists.
#[test]
fn a_process_that_cannot_be_signalled_still_exists() {
    assert!(target_exists(Target::Process(1)));
}
