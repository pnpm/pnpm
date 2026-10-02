pub use crate::shell_emulator_types::{EmulatedOutput, ShellEmulatorError};
use crate::{
    ProcessTracker,
    process::{Command, Stdio},
    spawn_child,
};
use pnpm_reporter::LifecycleStdio;
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    thread,
};

pub fn execute_emulated(
    script: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    output: EmulatedOutput<'_>,
    process_tracker: Option<&ProcessTracker>,
) -> Result<i32, ShellEmulatorError> {
    run(script, cwd, env, output, process_tracker)
        .map_err(|source| {
            if source.kind() == io::ErrorKind::InvalidData {
                ShellEmulatorError::Parse { script: script.to_owned(), message: source.to_string() }
            } else {
                ShellEmulatorError::Start { script: script.to_owned(), source }
            }
        })
}

fn run(
    script: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    output: EmulatedOutput<'_>,
    process_tracker: Option<&ProcessTracker>,
) -> io::Result<i32> {
    let mut command = Command::shell_emulator(script);
    command
        .current_dir(cwd)
        .env_clear()
        .envs(env);
    if matches!(output, EmulatedOutput::Lines(_)) {
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
    let mut child = spawn_child(&mut command, process_tracker)?;
    let status = match output {
        EmulatedOutput::Inherit => child.wait()?,
        EmulatedOutput::Lines(sink) => thread::scope(|scope| {
            let stdout = child
                .child_mut()
                .stdout
                .take()
                .expect("stdout is piped");
            let stderr = child
                .child_mut()
                .stderr
                .take()
                .expect("stderr is piped");
            let stdout = scope.spawn(move || pump_lines(stdout, LifecycleStdio::Stdout, sink));
            let stderr = scope.spawn(move || pump_lines(stderr, LifecycleStdio::Stderr, sink));
            let status = child.wait();
            join(stdout)?;
            join(stderr)?;
            status
        })?,
    };
    Ok(status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
}

fn join(worker: thread::ScopedJoinHandle<'_, io::Result<()>>) -> io::Result<()> {
    worker.join().unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

fn pump_lines(
    reader: impl Read,
    stdio: LifecycleStdio,
    sink: &(dyn Fn(LifecycleStdio, String) + Sync),
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        sink(stdio, String::from_utf8_lossy(&line).into_owned());
        line.clear();
    }
    Ok(())
}
