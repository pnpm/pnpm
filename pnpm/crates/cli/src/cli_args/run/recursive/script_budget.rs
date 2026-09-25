use super::{Condvar, Config, Mutex, RunArgs, TaskGraph, script_concurrency};

/// The run-wide ceiling on scripts in flight.
///
/// The task scheduler caps the tasks it dispatches, which caps the
/// scripts only as long as a task runs one script at a time. A
/// `/pattern/` selector gives a task several, so every script takes a
/// permit from this budget before it starts and frees it when it ends.
/// That is what keeps `workspaceConcurrency` a limit on the processes
/// pnpm spawns rather than on the tasks it dispatches.
pub(super) struct ScriptBudget {
    free: Mutex<usize>,
    freed: Condvar,
}

impl ScriptBudget {
    /// A budget of `limit` scripts at once, and of one when `limit` is
    /// zero: a run that starts nothing makes no progress.
    pub(super) fn new(limit: usize) -> Self {
        ScriptBudget { free: Mutex::new(limit.max(1)), freed: Condvar::new() }
    }

    /// Wait for a permit, then hold it until the returned guard drops.
    pub(super) fn acquire(&self) -> ScriptPermit<'_> {
        let mut free = self.free.lock().expect("script budget lock is not poisoned");
        while *free == 0 {
            free = self.freed.wait(free).expect("script budget lock is not poisoned");
        }
        *free -= 1;
        ScriptPermit { budget: self }
    }
}

/// One script's share of the run's [`ScriptBudget`].
pub(super) struct ScriptPermit<'a> {
    budget: &'a ScriptBudget,
}

impl Drop for ScriptPermit<'_> {
    fn drop(&mut self) {
        *self.budget.free.lock().expect("script budget lock is not poisoned") += 1;
        self.budget.freed.notify_one();
    }
}

/// The budget for a run of `task_graph`: every matched script at once
/// under `--parallel`, one at a time under `--sequential`, and
/// `workspaceConcurrency` otherwise.
pub(super) fn run_script_budget(
    args: &RunArgs,
    config: &Config,
    task_graph: &TaskGraph,
) -> ScriptBudget {
    let scripts = task_graph
        .values()
        .map(|node| node.scripts.len())
        .sum();
    ScriptBudget::new(script_concurrency(config, scripts, args.workspace.parallel, args.sequential))
}
