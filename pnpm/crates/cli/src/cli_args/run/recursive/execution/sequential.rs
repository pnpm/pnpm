use super::{
    Instant, ProjectExecution, ProjectScripts, RunContext, ScriptRunState, apply_script_result,
    reenters_running_script, run_stages, runnable_project_script, start_script,
};

/// Run the task's scripts one at a time, in selection order: the mode a
/// single-script task, `--sequential`, and a run whose script-level
/// concurrency is one all take.
pub(super) fn run_scripts(run: &ProjectScripts<'_, '_, '_>) -> miette::Result<ProjectExecution> {
    let options = run.options;
    let root = options.node.project.as_path();
    let mut state = ScriptRunState::queued();
    for selected in &options.node.scripts {
        let Some(script) = runnable_project_script(run.manifest, selected, options.args)? else {
            continue;
        };
        // Running the script pnpm is already inside would recurse; the
        // guard is what `pnpm -r test` from within a `test` script needs.
        if reenters_running_script(selected, root) {
            state.execution.recursion_guarded = true;
            continue;
        }

        let Some(permit) = start_script(&options.process) else {
            state.cancelled_script();
            break;
        };
        (options.process.on_started)();
        state.before_script();
        let ran = run_one_script(run, selected, &script, &mut state)?;
        drop(permit);
        // A cancelled run ends the project outright; `--bail` stops it
        // at the first script that failed.
        if !ran || (state.failed && options.process.bail) {
            break;
        }
    }
    Ok(state.execution)
}

/// Run one of the task's scripts and record its verdict. `Ok(false)`
/// when the run was cancelled, which ends the project outright.
fn run_one_script(
    run: &ProjectScripts<'_, '_, '_>,
    selected: &str,
    script: &str,
    state: &mut ScriptRunState,
) -> miette::Result<bool> {
    let options = run.options;
    let root = options.node.project.as_path();
    let ctx: RunContext<'_> = options.run_context(run.manifest, root, run.extra_env, run.root_str);
    let start = Instant::now();
    let status = run_stages(&ctx, selected, script, options.args.script_args())?;
    let duration = start.elapsed().as_secs_f64() * 1e3;
    Ok(apply_script_result(state, &ctx, status, duration))
}
