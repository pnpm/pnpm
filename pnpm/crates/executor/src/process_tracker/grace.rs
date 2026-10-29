//! The Unix side of a cancellation: `SIGTERM` now, and `SIGKILL` once a
//! grace has passed for whatever of the task is still provably the task.

use super::{
    RunningExecution, Stop, TrackerState,
    signal::{descendant_processes, group_has_member, in_signalled_group, signal},
};
use std::{
    sync::{Arc, Mutex, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long a cancelled task has to stop on `SIGTERM` before it is killed.
/// Other task runners force the stop after half a second (turborepo), two
/// seconds (moon), or five (Nx).
const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// Ask `executions` to stop and leave a thread to kill what ignores it. Every
/// descendant found through parent links gets `SIGTERM` too, except one that
/// the signal to its process group reached already: a second `SIGTERM` reads
/// as "quit now" to many tools.
pub(super) fn stop_gracefully(
    state: &Arc<Mutex<TrackerState>>,
    executions: Vec<(usize, RunningExecution)>,
) {
    let roots: Vec<Root> = executions
        .iter()
        .filter_map(|(id, execution)| {
            execution
                .signal_target()
                .map(|target| Root { id: *id, target, pinned: pins_its_pid(execution) })
        })
        .collect();
    let descendants = descendant_processes(&pinned_pids(&roots));
    for (_, execution) in executions {
        execution.stop();
    }
    let targets: Vec<i32> = roots
        .iter()
        .map(|root| root.target)
        .collect();
    for descendant in &descendants {
        if !in_signalled_group(descendant.pid(), &targets) {
            descendant.signal(Stop::Graceful);
        }
    }
    kill_after_grace(state, roots);
}

/// A cancelled execution: the tracker's id for it, what a signal for it
/// addresses (see [`RunningExecution::signal_target`]), and whether its pid
/// still named it at the cancellation.
#[derive(Clone, Copy)]
struct Root {
    id: usize,
    target: i32,
    pinned: bool,
}

/// Whether `execution` is a child its waiter has not reaped, whose pid
/// therefore still names it. A reaped child's group stays registered while
/// its waiter waits on the rest of the group, but its pid is free.
fn pins_its_pid(execution: &RunningExecution) -> bool {
    matches!(execution, RunningExecution::Process { .. })
}

/// The pids of the pinned `roots`, group leaders included.
fn pinned_pids(roots: &[Root]) -> Vec<u32> {
    roots
        .iter()
        .filter(|root| root.pinned)
        .map(|root| root.target.unsigned_abs())
        .collect()
}

/// What is left of the cancelled roots, as far as its identity is still
/// pinned: groups that still have a member, and roots their waiter has not
/// reaped, which therefore still hold their pid.
struct Survivors {
    groups: Vec<i32>,
    unreaped: Vec<u32>,
}

impl Survivors {
    fn find(state: &Mutex<TrackerState>, roots: &[Root]) -> Self {
        // A root reaped during the grace is still registered as its group,
        // so what is unreaped is what is still registered as a process.
        let unreaped: Vec<Root> = {
            let state = state.lock().unwrap_or_else(PoisonError::into_inner);
            roots
                .iter()
                .filter(|root| state.executions.get(&root.id).is_some_and(pins_its_pid))
                .map(|&root| Root { pinned: true, ..root })
                .collect()
        };
        let groups = roots
            .iter()
            .map(|root| root.target)
            .filter(|&target| target < 0 && group_has_member(target))
            .collect();
        Self { groups, unreaped: pinned_pids(&unreaped) }
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.unreaped.is_empty()
    }

    /// Kill the survivors and the descendants of the unreaped roots, as a
    /// scan finds them now rather than as they were at the cancellation, so
    /// no pid recycled in between is hit.
    fn kill(&self) {
        let descendants = descendant_processes(&self.unreaped);
        for &group in &self.groups {
            if group_has_member(group) {
                signal(group, Stop::Forced);
            }
        }
        let unreaped = self.unreaped
            .iter()
            .filter_map(|&pid| i32::try_from(pid).ok());
        for pid in unreaped {
            signal(pid, Stop::Forced);
        }
        for descendant in &descendants {
            descendant.signal(Stop::Forced);
        }
    }
}

/// The threads waiting out the grace of a cancellation, for
/// [`finish_cancellations`] to join.
static GRACE_WATCHES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// Kill what is left of `roots` once [`CANCEL_GRACE`] has passed, from a
/// thread of its own that ends as soon as nothing is left. A thread that
/// cannot be started kills them at once instead: waiting on a task that
/// ignores `SIGTERM` would never end.
///
/// Only what is still provably the cancelled task is killed (see
/// [`Survivors`]), since a pid recorded two seconds earlier may name another
/// process by then. A descendant whose chain of parents no longer leads to
/// an unreaped root, because a process in that chain exited during the
/// grace, is not found again and keeps running.
fn kill_after_grace(state: &Arc<Mutex<TrackerState>>, roots: Vec<Root>) {
    if roots.is_empty() {
        return;
    }
    let deadline = Instant::now() + CANCEL_GRACE;
    let roots: Arc<[Root]> = roots.into();
    let watch = {
        let state = Arc::clone(state);
        let roots = Arc::clone(&roots);
        move || watch_until(&state, &roots, deadline)
    };
    match thread::Builder::new().name("pnpm-cancel-grace".to_string()).spawn(watch) {
        Ok(handle) => GRACE_WATCHES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(handle),
        Err(_) => Survivors::find(state, &roots).kill(),
    }
}

/// Wait for `roots` to end, and kill what is left of them at `deadline`.
fn watch_until(state: &Mutex<TrackerState>, roots: &[Root], deadline: Instant) {
    loop {
        let survivors = Survivors::find(state, roots);
        if survivors.is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            survivors.kill();
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Wait until every task a cancellation stopped has ended, killing those
/// still running when their grace runs out. pnpm calls it on its way out,
/// so a task that ignores `SIGTERM` behind a shell that did not cannot
/// outlive the grace by outliving pnpm.
pub(crate) fn finish_cancellations() {
    let watches =
        std::mem::take(&mut *GRACE_WATCHES.lock().unwrap_or_else(PoisonError::into_inner));
    for watch in watches {
        let _ = watch.join();
    }
}
