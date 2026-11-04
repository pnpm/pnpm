//! Run a command whose stall should be diagnosable from a CI log.

use std::{
    fmt::Write as _,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, ExitStatus, Output, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Below nextest's `terminate-after`, so the report is printed before
/// nextest kills the test.
pub const STALL_DEADLINE: Duration = Duration::from_mins(4);

/// Run `command` the way [`Command::output`] does, with two additions:
///
/// - Each line it prints is echoed to this process's stderr, stamped with
///   the time since the start. The echo survives nextest killing the test.
/// - A command still running after `deadline` fails the test with the
///   processes it has running at that moment, which are then killed.
pub fn output_within(command: &mut Command, deadline: Duration) -> Output {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the command");
    let stdout = echo_lines(child.stdout.take().expect("piped stdout"), "stdout", started);
    let stderr = echo_lines(child.stderr.take().expect("piped stderr"), "stderr", started);
    let Some(status) = wait_until(&mut child, started + deadline) else {
        let report = process_tree(child.id());
        kill_tree(&mut child);
        panic!("the command was still running after {deadline:?}; its processes:\n{report}");
    };
    Output {
        status,
        stdout: stdout.join().expect("read the command's stdout"),
        stderr: stderr.join().expect("read the command's stderr"),
    }
}

#[expect(clippy::print_stderr, reason = "the echo is test output, which nextest captures")]
fn echo_lines(
    stream: impl Read + Send + 'static,
    label: &'static str,
    started: Instant,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut collected = Vec::new();
        let mut line = Vec::new();
        while reader
            .read_until(b'\n', &mut line)
            .is_ok_and(|read| read > 0)
        {
            let elapsed = started.elapsed().as_secs_f32();
            eprintln!("[{elapsed:>6.1}s {label}] {}", String::from_utf8_lossy(&line).trim_end());
            collected.append(&mut line);
        }
        collected
    })
}

fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait().expect("poll the command") {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// `root` and its live descendants, one per line, indented by depth.
fn process_tree(root: u32) -> String {
    let system = system_processes();
    let mut report = String::new();
    for (depth, process) in descendants(&system, Pid::from_u32(root)) {
        let cmd = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            report,
            "{:indent$}{} {:?} cpu={:.1}s up={}s: {cmd}",
            "",
            process.pid(),
            process.name(),
            process.accumulated_cpu_time() as f64 / 1000.0,
            process.run_time(),
            indent = depth * 2,
        );
    }
    report
}

fn kill_tree(child: &mut Child) {
    let system = system_processes();
    let mut tree = descendants(&system, Pid::from_u32(child.id()));
    tree.reverse();
    for (_, process) in tree {
        process.kill();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn system_processes() -> System {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_cmd(UpdateKind::Always),
    );
    system
}

/// `root` first, then each process below it after its parent. A process
/// that started before `root` is not one of its descendants, whatever its
/// parent pid says: Windows reuses the pid of a process that has exited.
fn descendants(system: &System, root: Pid) -> Vec<(usize, &Process)> {
    let Some(root_process) = system.process(root) else {
        return Vec::new();
    };
    let mut tree = vec![(0, root_process)];
    let mut index = 0;
    while let Some(&(depth, parent)) = tree.get(index) {
        let children = system
            .processes()
            .values()
            .filter(|process| {
                process.parent() == Some(parent.pid())
                    && process.start_time() >= parent.start_time()
            });
        let at = index + 1;
        let children: Vec<_> = children
            .map(|child| (depth + 1, child))
            .collect();
        tree.splice(at..at, children);
        index += 1;
    }
    tree
}

#[cfg(test)]
mod tests;
