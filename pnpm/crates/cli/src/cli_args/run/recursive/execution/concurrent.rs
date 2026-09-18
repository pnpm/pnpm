use super::{
    Instant, Mutex, ProcessTracker, ProjectExecution, ProjectScripts, RunContext, ScriptRunState,
    TaskCompletion, apply_script_result, reenters_running_script, run_stages,
    runnable_project_script,
};
use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pnpm_executor::ScriptExit;
use pnpm_workspace_task_scheduler::{ScheduleGraphOptions, schedule_graph};

/// Dispatch the task's scripts over the shared scheduler, then fold what
/// each one settled as into the task's single [`ProjectExecution`].
pub(super) fn run_scripts(run: &ProjectScripts<'_, '_, '_>) -> miette::Result<ProjectExecution> {
    let options = run.options;
    let root = options.node.project.as_path();
    let ctx = options.run_context(run.manifest, root, run.extra_env, run.root_str);
    let state = Mutex::new(ScriptRunState::queued());
    let abort: Mutex<Option<miette::Report>> = Mutex::new(None);
    let run_script = |selected: String| run_one_script(run, &ctx, &state, &abort, &selected);
    let tasks: IndexMap<String, Vec<String>> = options.node.scripts
        .iter()
        .cloned()
        .map(|name| (name, Vec::new()))
        .collect();
    let on_script_skipped = |_: &String| {};
    let schedule = ScheduleGraphOptions::new(
        run.concurrency,
        options.process.bail,
        &run_script,
        &on_script_skipped,
    )
    .continue_on_failure(!options.process.bail);
    schedule_graph(&tasks, &schedule).into_diagnostic()?;
    if let Some(error) = abort.into_inner().expect("run abort lock is not poisoned") {
        return Err(error);
    }
    Ok(state.into_inner().expect("run state lock is not poisoned").execution)
}

/// Run one of the task's scripts against the state the run's threads
/// share, recording its verdict there once it settles.
fn run_one_script(
    run: &ProjectScripts<'_, '_, '_>,
    ctx: &RunContext<'_>,
    state: &Mutex<ScriptRunState>,
    abort: &Mutex<Option<miette::Report>>,
    selected: &str,
) -> TaskCompletion {
    let Some(script) = (match runnable_project_script(run.manifest, selected, run.options.args) {
        Ok(script) => script,
        Err(error) => return abort_run(run.options.process.process_tracker, abort, error),
    }) else {
        return TaskCompletion::Passed;
    };
    // Running the script pnpm is already inside would recurse; the guard
    // is what `pnpm -r test` from within a `test` script needs.
    if reenters_running_script(selected, run.options.node.project.as_path()) {
        state.lock().expect("run state lock is not poisoned").execution.recursion_guarded = true;
        return TaskCompletion::Passed;
    }

    let _permit = run.options.process.script_budget.acquire();
    (run.options.process.on_started)();
    state
        .lock()
        .expect("run state lock is not poisoned")
        .before_script();
    let start = Instant::now();
    match run_stages(ctx, selected, &script, run.options.args.script_args()) {
        Ok(status) => settle_script(ctx, state, status, start.elapsed().as_secs_f64() * 1e3),
        Err(error) => abort_run(run.options.process.process_tracker, abort, error),
    }
}

fn settle_script(
    ctx: &RunContext<'_>,
    state: &Mutex<ScriptRunState>,
    status: ScriptExit,
    duration: f64,
) -> TaskCompletion {
    let mut state = state.lock().expect("run state lock is not poisoned");
    if !apply_script_result(&mut state, ctx, status, duration) {
        return TaskCompletion::Cancelled;
    }
    if state.failed {
        return TaskCompletion::Failed;
    }
    TaskCompletion::Passed
}

/// Hold the run's first infrastructure failure for rethrow and stop the
/// remaining scripts from dispatching, like `RunOutcome::abort` does
/// for a task. The tracker cancellation is what stops the siblings
/// still running beside the aborted script.
fn abort_run(
    process_tracker: Option<&ProcessTracker>,
    abort: &Mutex<Option<miette::Report>>,
    error: miette::Report,
) -> TaskCompletion {
    let mut abort = abort.lock().expect("run abort lock is not poisoned");
    if abort.is_none() {
        *abort = Some(error);
    }
    if let Some(process_tracker) = process_tracker {
        process_tracker.cancel();
    }
    TaskCompletion::Aborted
}
