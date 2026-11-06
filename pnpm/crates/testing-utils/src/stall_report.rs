//! Run a command whose stall should be diagnosable from a CI log.

use std::{
    cmp::Reverse,
    collections::HashSet,
    ffi::OsString,
    fmt::Write as _,
    io::Read,
    mem,
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use sysinfo::{
    Pid, Process, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind,
};

/// Below nextest's `terminate-after`, so the report is printed before
/// nextest kills the test.
pub const STALL_DEADLINE: Duration = Duration::from_mins(4);

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Set on the command to a value unique to the run. Every process the
/// command starts inherits it, so it is found after its parent has exited.
const RUN_MARKER: &str = "PNPM_TEST_STALL_REPORT_RUN";

/// Run `command` the way [`Command::output`] does, with two additions:
///
/// - Everything it prints is echoed to this process's stderr as it
///   arrives, stamped with the time since the start. The echo survives
///   nextest killing the test.
/// - The command is done once it has exited and its output streams have
///   closed. A command not done after `deadline` fails the test with the
///   output so far and the processes of the run that are still running,
///   which are then killed.
pub fn output_within(command: &mut Command, deadline: Duration) -> Output {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let run_id = format!("{}-{}", std::process::id(), RUNS.fetch_add(1, Ordering::Relaxed));
    let run = OsString::from(format!("{RUN_MARKER}={run_id}"));
    let started = Instant::now();
    let mut child = command
        .env(RUN_MARKER, &run_id)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the command");
    let stdout = EchoedStream::start(child.stdout.take().expect("piped stdout"), "stdout", started);
    let stderr = EchoedStream::start(child.stderr.take().expect("piped stderr"), "stderr", started);
    let mut status = None;
    while started.elapsed() < deadline {
        if status.is_none() {
            status = child.try_wait().expect("poll the command");
        }
        if status.is_some() && stdout.is_closed() && stderr.is_closed() {
            let status = child.wait().expect("read the exit status of the command");
            return Output { status, stdout: stdout.take(), stderr: stderr.take() };
        }
        thread::sleep(POLL_INTERVAL);
    }
    fail_stalled(Stall { child, run, status, stdout, stderr, deadline })
}

/// A command that was not done by its deadline.
struct Stall {
    child: Child,
    run: OsString,
    /// [`None`] while the command itself is still running.
    status: Option<ExitStatus>,
    stdout: EchoedStream,
    stderr: EchoedStream,
    deadline: Duration,
}

fn fail_stalled(stall: Stall) -> ! {
    let Stall {
        mut child,
        run,
        status,
        stdout,
        stderr,
        deadline,
    } = stall;
    let root = status.is_none().then(|| Pid::from_u32(child.id()));
    let report = RunProcesses::find(
        &run,
        root,
        ProcessRefreshKind::nothing().with_cpu().with_cmd(UpdateKind::Always),
    )
    .report();
    RunProcesses::kill(&run, root);
    let _ = child.kill();
    let _ = child.wait();
    let state = match status {
        Some(status) => {
            format!("exited with {status}, but the processes it started kept its output open")
        }
        None => "was still running".to_string(),
    };
    panic!(
        "the command {state} after {deadline:?}; its processes:\n{report}{}\
         stdout so far:\n{}\nstderr so far:\n{}",
        crate::registry_activity::report(),
        String::from_utf8_lossy(&stdout.take()),
        String::from_utf8_lossy(&stderr.take()),
    );
}

struct EchoedStream {
    collected: Arc<Mutex<Vec<u8>>>,
    reader: JoinHandle<()>,
}

impl EchoedStream {
    /// Echo each chunk as it arrives, so output that does not end in a
    /// newline is echoed before the command stalls.
    fn start(
        mut stream: impl Read + Send + 'static,
        label: &'static str,
        started: Instant,
    ) -> Self {
        let collected = Arc::<Mutex<Vec<u8>>>::default();
        let reader = thread::spawn({
            let collected = Arc::clone(&collected);
            move || {
                let mut chunk = [0; 8192];
                while let Ok(read @ 1..) = stream.read(&mut chunk) {
                    echo(&chunk[..read], label, started);
                    lock(&collected).extend_from_slice(&chunk[..read]);
                }
            }
        });
        EchoedStream { collected, reader }
    }

    fn is_closed(&self) -> bool {
        self.reader.is_finished()
    }

    fn take(&self) -> Vec<u8> {
        mem::take(&mut lock(&self.collected))
    }
}

#[expect(clippy::print_stderr, reason = "the echo is test output, which nextest captures")]
fn echo(chunk: &[u8], label: &str, started: Instant) {
    let elapsed = started.elapsed().as_secs_f32();
    for line in String::from_utf8_lossy(chunk).lines() {
        eprintln!("[{elapsed:>6.1}s {label}] {line}");
    }
}

fn lock<Value>(mutex: &Mutex<Value>) -> std::sync::MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The running processes of a run: the ones whose environment carries its
/// [`RUN_MARKER`], the command's own process while it runs, and the
/// processes those started. A process that clears its environment is
/// found only while its parent runs.
struct RunProcesses {
    system: System,
    members: HashSet<Pid>,
}

impl RunProcesses {
    /// `details` is what to read of each process beyond its environment.
    fn find(run: &OsString, root: Option<Pid>, details: ProcessRefreshKind) -> Self {
        let mut system = System::new().expect("read the process table");
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            details.with_environ(UpdateKind::Always),
        );
        let mut members: HashSet<Pid> = system
            .processes()
            .values()
            .filter(|process| process.environ().contains(run))
            .map(Process::pid)
            .chain(root)
            .collect();
        // A process started before its parent is not its child, whatever its
        // parent pid says: Windows reuses the pid of a process that has exited.
        loop {
            let children: Vec<_> = system
                .processes()
                .values()
                .filter(|process| !members.contains(&process.pid()))
                .filter(|process| {
                    process
                        .parent()
                        .and_then(|parent| system.process(parent))
                        .is_some_and(|parent| {
                            members.contains(&parent.pid())
                                && process.start_time() >= parent.start_time()
                        })
                })
                .map(Process::pid)
                .collect();
            if children.is_empty() {
                break;
            }
            members.extend(children);
        }
        // A zombie has exited and cannot be killed; on Linux it stays until
        // an init process that may never reap it does.
        members.retain(|pid| {
            system
                .process(*pid)
                .is_some_and(|process| process.status() != ProcessStatus::Zombie)
        });
        RunProcesses { system, members }
    }

    /// Kill every process of the run, and then whatever they started
    /// meanwhile.
    fn kill(run: &OsString, root: Option<Pid>) {
        for _ in 0..5 {
            let processes = Self::find(run, root, ProcessRefreshKind::nothing());
            if processes.members.is_empty() {
                return;
            }
            for pid in &processes.members {
                if let Some(process) = processes.system.process(*pid) {
                    process.kill();
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Each process after its parent, with the number of its ancestors in
    /// the run.
    fn tree(&self) -> Vec<(usize, &Process)> {
        let mut processes: Vec<_> = self.members
            .iter()
            .filter_map(|&pid| self.system.process(pid))
            .collect();
        // Reversed, so the stack below pops siblings in pid order.
        processes.sort_by_key(|process| Reverse(process.pid()));
        let mut tree = Vec::new();
        let mut pending: Vec<_> = processes
            .iter()
            .filter(|process| self.is_top_level(process))
            .map(|&process| (0, process))
            .collect();
        while let Some((depth, process)) = pending.pop() {
            tree.push((depth, process));
            let children = processes
                .iter()
                .filter(|child| child.parent() == Some(process.pid()));
            pending.extend(children.map(|&child| (depth + 1, child)));
        }
        tree
    }

    fn is_top_level(&self, process: &Process) -> bool {
        process
            .parent()
            .is_none_or(|parent| !self.members.contains(&parent))
    }

    fn report(&self) -> String {
        let mut report = String::new();
        for (depth, process) in self.tree() {
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
}

#[cfg(test)]
mod tests;
