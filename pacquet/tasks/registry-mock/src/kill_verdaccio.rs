use pipe_trait::Pipe;
use std::collections::HashSet;
use sysinfo::{Pid, Process, ProcessRefreshKind, RefreshKind, Signal, System};

/// Walk the parent chain iteratively to decide whether `process`
/// descends from `suspect_ancestor`.
///
/// Two reasons this is a loop rather than the obvious recursion:
///
/// 1. **Stack safety on Windows.** A 1 MiB default thread stack
///    overflowed during cleanup on `windows-latest` (#296 CI: a
///    `STATUS_STACK_OVERFLOW` from `pacquet-registry-mock end` after
///    every test passed). Iterating uses constant stack regardless
///    of process-tree depth.
/// 2. **Cycle safety.** `Process::parent()` on Windows reports
///    `dwParentProcessID` as recorded at process-create time and is
///    never updated when the parent dies — so the recorded parent
///    PID can be reused by an unrelated later process and, in the
///    pathological case, observably "loop back" through the snapshot.
///    A `visited` set bounds the walk regardless.
fn is_descent_of(process: &Process, suspect_ancestor: Pid, system: &System) -> bool {
    let mut current_parent = process.parent();
    let mut visited: HashSet<Pid> = HashSet::new();
    while let Some(parent_pid) = current_parent {
        if parent_pid == suspect_ancestor {
            return true;
        }
        if !visited.insert(parent_pid) {
            // Cycle in the snapshot's parent chain — no need to walk
            // further; if `suspect_ancestor` were on this loop we'd
            // have hit it already.
            return false;
        }
        current_parent = system.processes().get(&parent_pid).and_then(|p| p.parent());
    }
    false
}

pub fn kill_all_verdaccio_children_in(root: Pid, signal: Signal, system: &System) -> usize {
    system
        .processes()
        .values()
        .filter(|process| is_descent_of(process, root, system))
        .filter(|process| process.kill_with(signal).unwrap_or_else(|| process.kill()))
        .count()
}

pub fn kill_all_verdaccio_children(root: Pid, signal: Signal) -> usize {
    let system = RefreshKind::nothing()
        .with_processes(ProcessRefreshKind::nothing())
        .pipe(System::new_with_specifics);
    kill_all_verdaccio_children_in(root, signal, &system)
}
