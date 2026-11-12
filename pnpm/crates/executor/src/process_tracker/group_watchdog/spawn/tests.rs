use super::{Command, GroupWatchdog, registered};
use std::{io::Read, process::Stdio};

#[test]
fn failed_exec_releases_the_group_registered_before_exec() {
    let mut watchdog = recording_watchdog();
    let mut command = Command::new("/nonexistent-pnpm-test-executable");
    command.process_group(0);
    let error = registered(&mut command, &mut watchdog).expect_err("exec should fail");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    let lines = recorded(watchdog);
    let [added, removed] = lines.lines().collect::<Vec<_>>()[..] else {
        panic!("expected registration and release: {lines:?}");
    };
    assert!(added.starts_with('+'));
    assert_eq!(removed, format!("-{}", &added[1..]));
}

#[test]
fn reusing_a_command_registers_each_child_once() {
    let mut watchdog = recording_watchdog();
    let mut command = Command::new("true");
    command.process_group(0);
    let mut leaders = Vec::new();
    for _ in 0..3 {
        let mut child = registered(&mut command, &mut watchdog).expect("spawn child");
        leaders.push(format!("+{}", child.id()));
        assert!(
            child
                .wait()
                .expect("wait for child")
                .success(),
        );
    }
    assert!(
        command
            .status()
            .expect("spawn without registration")
            .success(),
    );
    let lines = recorded(watchdog);
    assert_eq!(lines.lines().collect::<Vec<_>>(), leaders);
}

fn recording_watchdog() -> GroupWatchdog {
    let mut process = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn recorder");
    let lifeline = process.stdin.take().unwrap();
    GroupWatchdog { process, lifeline }
}

fn recorded(watchdog: GroupWatchdog) -> String {
    let GroupWatchdog { mut process, lifeline } = watchdog;
    drop(lifeline);
    let mut output = String::new();
    process.stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(process.wait().unwrap().success());
    output
}
