//! Windows console events reach every attached process at once, so pnpm
//! relays nothing. But a `cmd` hosting a batch script answers the event
//! with a "Terminate batch job (Y/N)?" prompt and waits on the answer
//! forever, holding the terminal hostage while pnpm waits on it
//! ([pnpm/pnpm#14860](https://github.com/pnpm/pnpm/issues/14860)). These
//! tests hand the interrupt to their own child's relay entry directly, so
//! no real console is needed and the children other tests register are
//! never touched: a `cmd` reading an answer from a pipe pnpm never writes
//! to stands in for the one at that prompt.
use super::{STATUS_CONTROL_C_EXIT, WINDOWS_INTERRUPT_GRACE, interrupt_child};
use crate::interrupt::{SignalRelay, relay_to_child};
use pretty_assertions::assert_eq;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{OnceLock, atomic::Ordering},
    thread::sleep,
    time::{Duration, Instant},
};

/// The node binary itself. The `node` on `PATH` can be a shim that runs
/// it as a child process, and a child with a child process of its own is
/// one pnpm keeps waiting for.
fn node_binary() -> &'static Path {
    static NODE: OnceLock<PathBuf> = OnceLock::new();
    NODE.get_or_init(|| {
        let output = Command::new("node")
            .args(["-p", "process.execPath"])
            .stderr(Stdio::inherit())
            .output()
            .expect("run node");
        assert!(output.status.success(), "node reports its path");
        PathBuf::from(String::from_utf8(output.stdout).expect("a UTF-8 path").trim())
    })
}

fn spawn_node(script: &str) -> Child {
    Command::new(node_binary())
        .args(["-e", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn node")
}

/// A `cmd` running `node -e script` the way pnpm's default script shell
/// runs a script, and exiting with its code.
fn spawn_cmd_running_node(script: &str) -> Child {
    Command::new("cmd")
        .raw_arg(format!(r#"/d /s /c ""{}" -e "{script}"""#, node_binary().display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cmd")
}

/// A `cmd` waiting on an answer with no child process under it, as it
/// does at its batch-termination prompt. Its stdin stays open, and so
/// unanswered, for as long as the returned child is held.
fn spawn_stuck_cmd() -> Child {
    Command::new("cmd")
        .args(["/d", "/c", "set /p answer="])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cmd")
}

/// Deliver the console's interrupt to the child `relay` registered, the
/// way the console event handler does for each registered child.
fn press_ctrl_c(relay: &SignalRelay) {
    let entry = relay.entry.expect("the child is registered");
    let target = entry.target.load(Ordering::Acquire);
    assert!(interrupt_child(entry, target), "pnpm keeps waiting for the child");
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
fn a_stuck_cmd_is_terminated_once_the_grace_passes() {
    let mut child = spawn_stuck_cmd();
    let relay = relay_to_child(child.id(), false);

    press_ctrl_c(&relay);
    let status = wait_for_exit(&mut child, WINDOWS_INTERRUPT_GRACE + Duration::from_secs(30));

    assert_eq!(
        status.code(),
        Some(STATUS_CONTROL_C_EXIT as i32),
        "the child reads as ended by the interrupt, not by an unrelated code",
    );
}

#[test]
fn a_cmd_that_exits_during_the_grace_keeps_its_own_exit_code() {
    let mut child = spawn_cmd_running_node("setTimeout(() => process.exit(42), 200)");
    let relay = relay_to_child(child.id(), false);

    press_ctrl_c(&relay);
    let status = wait_for_exit(&mut child, Duration::from_secs(30));

    assert_eq!(
        status.code(),
        Some(42),
        "a child shutting down on its own is not terminated over it",
    );
}

#[test]
fn a_cmd_whose_script_is_still_shutting_down_is_waited_for() {
    let shutdown_ms = (WINDOWS_INTERRUPT_GRACE * 3).as_millis();
    let mut child =
        spawn_cmd_running_node(&format!("setTimeout(() => process.exit(42), {shutdown_ms})"));
    let relay = relay_to_child(child.id(), false);

    press_ctrl_c(&relay);
    let status = wait_for_exit(&mut child, Duration::from_secs(30));

    assert_eq!(
        status.code(),
        Some(42),
        "a child whose script is still running is not cut off after the grace",
    );
}

#[test]
fn a_child_that_is_not_cmd_is_waited_for_without_child_processes() {
    // A shell such as PowerShell runs its cleanup in its own process, with
    // nothing under it, and has no batch-termination prompt to get stuck on.
    let shutdown_ms = (WINDOWS_INTERRUPT_GRACE * 3).as_millis();
    let mut child = spawn_node(&format!("setTimeout(() => process.exit(42), {shutdown_ms})"));
    let relay = relay_to_child(child.id(), false);

    press_ctrl_c(&relay);
    let status = wait_for_exit(&mut child, Duration::from_secs(30));

    assert_eq!(status.code(), Some(42), "only a `cmd` is ended after the grace");
}

#[test]
fn a_second_interrupt_terminates_without_waiting_for_the_grace() {
    let mut child = spawn_stuck_cmd();
    let relay = relay_to_child(child.id(), false);

    press_ctrl_c(&relay);
    press_ctrl_c(&relay);
    // The grace is a full second; the second press ends the child at once.
    let status = wait_for_exit(&mut child, Duration::from_millis(500));

    assert_eq!(status.code(), Some(STATUS_CONTROL_C_EXIT as i32));
}

#[test]
fn an_interrupt_for_a_released_entry_is_not_counted() {
    let mut child = spawn_node("setInterval(() => {}, 1000)");
    let relay = relay_to_child(child.id(), false);
    let entry = relay.entry.expect("the child is registered");
    let target = entry.target.load(Ordering::Acquire);
    let relays = entry.relays.load(Ordering::Relaxed);
    drop(relay);

    // A console event that read the target before the release arrives
    // after it, when the entry may already belong to another child.
    assert!(!interrupt_child(entry, target), "the released child is not waited for");
    let counted =
        entry.target.load(Ordering::Acquire) == 0 && entry.relays.load(Ordering::Relaxed) != relays;
    child.kill().expect("end the child");
    let _ = child.wait();

    assert!(!counted, "the stale event does not count against the entry");
}
