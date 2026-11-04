use super::{output_within, system_processes};
use std::{
    panic,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use sysinfo::Pid;

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
    let grandchild = "setTimeout(() => {}, 120000)";
    let child = format!(
        "const {{ spawn }} = require('node:child_process'); \
         const grandchild = spawn(process.execPath, ['-e', {grandchild:?}], {{ stdio: 'inherit' }}); \
         console.log(grandchild.pid)",
    );
    let panic = panic::catch_unwind(|| {
        output_within(Command::new("node").args(["-e", &child]), Duration::from_secs(5))
    })
    .expect_err("the command must fail past its deadline");
    let message = panic.downcast_ref::<String>().expect("a formatted panic message");

    assert!(message.contains("still running after 5s"));
    assert!(message.contains(grandchild), "the report must list the grandchild");
    let grandchild_pid = message
        .lines()
        .find(|line| line.contains(grandchild) && !line.contains("spawn"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|pid| pid.parse().ok())
        .expect("the report names the grandchild's pid");
    let deadline = Instant::now() + Duration::from_secs(10);
    while system_processes()
        .process(Pid::from_u32(grandchild_pid))
        .is_some()
    {
        assert!(Instant::now() < deadline, "the grandchild must be killed");
        thread::sleep(Duration::from_millis(100));
    }
}
