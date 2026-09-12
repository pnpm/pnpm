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
