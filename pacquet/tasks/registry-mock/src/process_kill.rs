use pipe_trait::Pipe;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

/// Send `signal` to the process at `pid`. Returns `true` if the
/// process existed and the signal was delivered, `false` if the
/// process was already gone or the signal couldn't be sent.
///
/// pnpr runs as a single process with no spawned children,
/// so we send the signal directly to the recorded PID instead of
/// walking the process tree.
pub fn kill_process_by_pid(pid: Pid, signal: Signal) -> bool {
    let system = RefreshKind::nothing()
        .with_processes(ProcessRefreshKind::nothing())
        .pipe(System::new_with_specifics);
    system
        .processes()
        .get(&pid)
        .is_some_and(|process| process.kill_with(signal).unwrap_or_else(|| process.kill()))
}
