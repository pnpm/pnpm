//! Windows console events reach every attached process at once, so pnpm
//! relays nothing. But a `cmd` hosting a batch script answers the event
//! with a "Terminate batch job (Y/N)?" prompt and waits on the answer
//! forever, holding the terminal hostage while pnpm waits on it
//! ([pnpm/pnpm#14860](https://github.com/pnpm/pnpm/issues/14860)). These
//! tests drive the console event handler directly, so no real console is
//! needed: a child that never exits on its own stands in for that `cmd`.
#![cfg(windows)]

use super::{STATUS_CONTROL_C_EXIT, WINDOWS_INTERRUPT_GRACE, relay_console_event, relay_to_child};
use pretty_assertions::assert_eq;
use std::{
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Mutex, MutexGuard},
    thread::sleep,
    time::{Duration, Instant},
};

const CTRL_C_EVENT: u32 = 0;

/// The relay list is process-wide, so tests that fire the console event
/// must not run concurrently: one test's event would visit another test's
/// entry and could terminate its child.
static RELAY_LOCK: Mutex<()> = Mutex::new(());

fn lock_relay() -> MutexGuard<'static, ()> {
    RELAY_LOCK.lock().expect("relay lock is not poisoned")
}

/// A child that never exits on its own stands in for a `cmd` waiting on
/// its batch-termination answer.
fn spawn_stuck_child() -> Child {
    Command::new("node")
        .args(["-e", "setInterval(() => {}, 1000)"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn a child that never exits on its own")
}

/// Send pnpm's console event handler the event the console would deliver.
fn press_ctrl_c() {
    // SAFETY: the handler only touches this module's relay list, and the
    // test drives it the way the console would.
    let handled = unsafe { relay_console_event(CTRL_C_EVENT) };
    assert_eq!(handled, 1, "the event is handled while a child is running");
}

/// The child's exit, or a failure once `deadline` passes.
fn wait_for_exit(child: &mut Child, deadline: Duration) -> ExitStatus {
    let deadline = Instant::now() + deadline;
    loop {
        if let Some(status) = child.try_wait().expect("poll the child") {
            return status;
        }
        assert!(Instant::now() < deadline, "the child should have exited");
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_interrupted_child_is_terminated_once_the_grace_passes() {
    let _lock = lock_relay();
    let mut child = spawn_stuck_child();
    let _relay = relay_to_child(child.id(), false);

    press_ctrl_c();
    let status = wait_for_exit(&mut child, WINDOWS_INTERRUPT_GRACE + Duration::from_secs(30));

    assert_eq!(
        status.code(),
        Some(STATUS_CONTROL_C_EXIT as i32),
        "the child reads as ended by the interrupt, not by an unrelated code",
    );
}

#[test]
fn a_child_that_exits_during_the_grace_keeps_its_own_exit_code() {
    let _lock = lock_relay();
    let mut child = Command::new("node")
        .args(["-e", "setTimeout(() => process.exit(42), 200)"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn a child that exits during the grace");
    let _relay = relay_to_child(child.id(), false);

    press_ctrl_c();
    let status = wait_for_exit(&mut child, Duration::from_secs(30));

    assert_eq!(
        status.code(),
        Some(42),
        "a child shutting down on its own is not terminated over it",
    );
}

#[test]
fn a_second_interrupt_terminates_without_waiting_for_the_grace() {
    let _lock = lock_relay();
    let mut child = spawn_stuck_child();
    let _relay = relay_to_child(child.id(), false);

    press_ctrl_c();
    press_ctrl_c();
    // The grace is a full second; the second press ends the child at once.
    let status = wait_for_exit(&mut child, Duration::from_millis(500));

    assert_eq!(status.code(), Some(STATUS_CONTROL_C_EXIT as i32));
}
