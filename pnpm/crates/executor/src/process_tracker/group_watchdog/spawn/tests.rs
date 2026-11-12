use super::{super::WatchdogProcess, Command, GroupWatchdog, registered};
use std::{io::Read, process::Stdio};

#[test]
fn failed_exec_releases_the_group_registered_before_exec() {
    let watchdog = recording_watchdog();
    let mut command = Command::new("/nonexistent-pnpm-test-executable");
    command.process_group(0);
    let error = registered(&mut command, &watchdog).expect_err("exec should fail");
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
    let watchdog = recording_watchdog();
    let mut command = Command::new("true");
    command.process_group(0);
    let mut leaders = Vec::new();
    for _ in 0..3 {
        let mut child = registered(&mut command, &watchdog).expect("spawn child");
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
    GroupWatchdog { process: std::sync::Mutex::new(WatchdogProcess::new(process)), lifeline }
}

fn recorded(watchdog: GroupWatchdog) -> String {
    let GroupWatchdog { process, lifeline } = watchdog;
    let mut process = process.into_inner().unwrap().child;
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

#[test]
fn releasing_a_group_does_not_wait_for_another_child_to_exec() {
    use crate::process_tracker::group_watchdog::{release, spawn};
    use std::{
        io::{self, Write},
        os::fd::AsRawFd,
        sync::mpsc,
        thread,
        time::Duration,
    };

    let mut first_command = Command::new("sleep");
    first_command.arg("30").process_group(0);
    let mut first = spawn(&mut first_command).unwrap();
    let leader = first.child.id();
    let (mut ready_reader, ready_writer) = io::pipe().unwrap();
    let (gate_reader, mut gate_writer) = io::pipe().unwrap();
    let pending = thread::spawn(move || {
        let mut command = Command::new("true");
        command.process_group(0);
        let ready = ready_writer.as_raw_fd();
        let gate = gate_reader.as_raw_fd();
        // SAFETY: both descriptors stay open through spawn. The callback uses
        // only async-signal-safe read/write calls with stack buffers.
        unsafe {
            command.pre_exec(move || wait_for_gate(ready, gate));
        }
        let mut child = spawn(&mut command).unwrap();
        assert!(child.child.wait().unwrap().success());
        release(child.child.id());
    });
    ready_reader
        .read_exact(&mut [0])
        .unwrap();
    let (sent, received) = mpsc::channel();
    let releasing = thread::spawn(move || {
        release(leader);
        sent.send(()).unwrap();
    });
    let completed = received.recv_timeout(Duration::from_secs(10));
    gate_writer.write_all(&[1]).unwrap();
    pending.join().unwrap();
    releasing.join().unwrap();
    first.child.kill().unwrap();
    first.child.wait().unwrap();
    completed.expect("release should not wait for the pending exec");
}

fn wait_for_gate(ready: std::os::fd::RawFd, gate: std::os::fd::RawFd) -> std::io::Result<()> {
    let mut byte = 1u8;
    // SAFETY: the stack buffers are valid for these async-signal-safe calls.
    let succeeded = unsafe {
        libc::write(ready, (&raw const byte).cast(), 1) == 1
            && libc::read(gate, (&raw mut byte).cast(), 1) == 1
    };
    if succeeded { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}
