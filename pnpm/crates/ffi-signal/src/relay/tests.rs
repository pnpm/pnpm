use super::{Target, process_group_of, target_exists, try_signal_target};

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

/// Signal `0` delivers nothing, so the reported outcome is the kernel's
/// answer about the target rather than anything the signal did.
#[test]
fn a_live_target_reports_a_sent_signal() {
    assert!(try_signal_target(Target::Process(std::process::id()), 0).is_ok());
}

/// Pins that the failure reaches the caller as the error the call met,
/// rather than as a bare "no". A reaped child's id names no process, so
/// the kernel answers `ESRCH`.
#[test]
fn an_absent_target_reports_why_nothing_was_sent() {
    let mut child = std::process::Command::new("true").spawn().expect("spawn a child to reap");
    let pid = child.id();
    child.wait().expect("reap the child");

    let error =
        try_signal_target(Target::Process(pid), 0).expect_err("the reaped pid names no process");

    assert_eq!(error.raw_os_error(), Some(libc::ESRCH));
}

/// An id too large for the kernel's own type reaches no call, so the
/// failure is the caller's input rather than anything the kernel reported.
#[test]
fn an_unaddressable_target_reports_a_rejected_input() {
    let error = try_signal_target(Target::Process(u32::MAX), 0).expect_err("u32::MAX is no pid");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(error.raw_os_error(), None, "the error is not one the kernel reported");
}
