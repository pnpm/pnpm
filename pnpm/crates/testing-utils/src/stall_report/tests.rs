use super::output_within;
use std::{
    panic,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

const LONG_RUNNING: &str = "setTimeout(() => {}, 120000)";

#[test]
fn a_command_that_finishes_returns_its_output() {
    let output = output_within(
        Command::new("node")
            .args(["-e", "process.stdout.write('out'); process.stderr.write('err')"]),
        Duration::from_mins(1),
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
}

#[test]
fn a_command_past_its_deadline_reports_and_ends_its_processes() {
    let message = stall_message(&spawn_grandchild_script(Parent::Waits));
    assert!(message.contains("was still running after 5s"));
    assert_grandchild_reported_and_killed(&message);
}

#[test]
fn a_command_whose_output_a_descendant_holds_open_is_not_done() {
    let message = stall_message(&spawn_grandchild_script(Parent::Exits));
    assert!(message.contains("exited with"), "the command itself exits");
    assert!(message.contains("kept its output open"));
    assert_grandchild_reported_and_killed(&message);
}

#[test]
fn a_stall_report_keeps_output_without_a_trailing_newline() {
    let message =
        stall_message(&format!("process.stdout.write('partial progress'); {LONG_RUNNING}"));
    assert!(message.contains("stdout so far:\npartial progress"));
}

#[derive(Clone, Copy)]
enum Parent {
    Waits,
    Exits,
}

/// A script that starts a long-running grandchild sharing its output.
fn spawn_grandchild_script(parent: Parent) -> String {
    // On Windows, libuv ends a child that is not detached when its parent
    // exits.
    let (options, then) = match parent {
        Parent::Waits => ("", ""),
        Parent::Exits => (", detached: true", "grandchild.unref()"),
    };
    format!(
        "const {{ spawn }} = require('node:child_process'); \
         const grandchild = spawn(process.execPath, ['-e', {LONG_RUNNING:?}], {{ stdio: 'inherit'{options} }}); \
         {then}",
    )
}

fn stall_message(script: &str) -> String {
    let panic = panic::catch_unwind(|| {
        output_within(Command::new("node").args(["-e", script]), Duration::from_secs(5))
    })
    .expect_err("the command must fail past its deadline");
    panic
        .downcast_ref::<String>()
        .expect("a formatted panic message")
        .clone()
}

fn assert_grandchild_reported_and_killed(message: &str) {
    let grandchild_pid = message
        .lines()
        .find(|line| line.contains(LONG_RUNNING) && !line.contains("spawn"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|pid| pid.parse().ok())
        .unwrap_or_else(|| panic!("the report must list the grandchild:\n{message}"));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut system = System::new().expect("read the process table");
    loop {
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        // A killed orphan can stay a zombie on Linux until init reaps it.
        let gone = system
            .process(Pid::from_u32(grandchild_pid))
            .is_none_or(|process| process.status() == ProcessStatus::Zombie);
        if gone {
            return;
        }
        assert!(Instant::now() < deadline, "the grandchild must be killed");
        thread::sleep(Duration::from_millis(100));
    }
}
