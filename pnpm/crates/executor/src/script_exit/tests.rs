use super::ScriptExit;

#[test]
fn emulated_zero_is_success() {
    assert!(ScriptExit::Emulated(0).success());
    assert_eq!(ScriptExit::Emulated(0).code(), Some(0));
}

#[test]
fn emulated_non_zero_reports_its_code() {
    assert!(!ScriptExit::Emulated(3).success());
    assert_eq!(ScriptExit::Emulated(3).code(), Some(3));
}

/// The `ScriptFailed` lifecycle error interpolates the exit, so an
/// emulated run has to read like the `ExitStatus` a spawned shell gives.
#[test]
fn emulated_renders_like_an_exit_status() {
    assert_eq!(ScriptExit::Emulated(1).to_string(), "exit status: 1");
}

#[cfg(unix)]
#[test]
fn a_signalled_child_reports_the_signal_name() {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    let killed = ScriptExit::Process(ExitStatus::from_raw(libc::SIGKILL));
    assert_eq!(killed.code(), None);
    assert_eq!(killed.signal_name(), Some("SIGKILL"));
    assert_eq!(ScriptExit::Process(ExitStatus::from_raw(1 << 8)).signal_name(), None);
    assert_eq!(ScriptExit::Emulated(1).signal_name(), None);
}
