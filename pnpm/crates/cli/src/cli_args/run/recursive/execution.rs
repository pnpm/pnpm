use super::{
    AtomicUsize, Config, ExecutionStatus, GraphPkg, HashMap, IndexMap, Instant, LogEvent, Mutex,
    Ordering, Path, ProcessTracker, ProjectGraph, RecursiveRun, RunArgs, RunContext, RunResults,
    ScriptBudget, ScriptOutput, ScriptPermit, Status, TaskCompletion, TaskGraph, TaskKey, TaskNode,
    env, make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution, run_stages, script_concurrency, task_summary_key,
};

/// The slots the tasks record into while they run.
pub(super) struct RunSlots {
    pub(super) result: Mutex<IndexMap<String, ExecutionStatus>>,
    pub(super) has_command: AtomicUsize,
    pub(super) first_failure: Mutex<Option<String>>,
    pub(super) abort: Mutex<Option<miette::Report>>,
}

impl RunSlots {
    pub(super) fn queued(task_graph: &TaskGraph) -> Self {
        RunSlots {
            result: Mutex::new(
                task_graph
                    .values()
                    .map(|node| (task_summary_key(node), ExecutionStatus::queued()))
                    .collect(),
            ),
            has_command: AtomicUsize::new(0),
            first_failure: Mutex::new(None),
            abort: Mutex::new(None),
        }
    }

    pub(super) fn into_results(self, bail: bool) -> miette::Result<RunResults> {
        if let Some(error) = self.abort.into_inner().expect("abort slot lock is not poisoned") {
            return Err(error);
        }
        let first_failure =
            self.first_failure.into_inner().expect("first-failure lock is not poisoned");
        Ok(RunResults {
            statuses: self.result.into_inner().expect("summary lock is not poisoned"),
            ran_a_command: self.has_command.load(Ordering::Relaxed) > 0,
            first_failure: bail.then_some(first_failure).flatten(),
        })
    }
}

/// Runs one task's project against the run's shared state.
pub(super) struct TaskRunner<'a, 'run, 'project> {
    pub(super) run: &'a RecursiveRun<'run, 'project>,
    pub(super) outcome: RunOutcome<'a>,
    pub(super) extra_env: &'a HashMap<String, String>,
    pub(super) bail: bool,
    pub(super) script_budget: &'a ScriptBudget,
    /// pnpm pipes unless the output cannot interleave: `--stream` off, and
    /// the graph cannot put two scripts in flight at once.
    pub(super) inherit_output: bool,
    pub(super) init_cwd: &'a Path,
}

impl TaskRunner<'_, '_, '_> {
    pub(super) fn run_task(&self, node: &TaskNode) -> TaskCompletion {
        let summary_key = task_summary_key(node);
        let on_started = || {
            self.outcome.result.lock().expect("summary lock is not poisoned")[&summary_key]
                .status = Status::Running;
        };
        let execution = run_project(&RunProjectOptions {
            node,
            graph: self.run.graph,
            args: self.run.args,
            init_cwd: self.init_cwd,
            config: self.run.config,
            extra_env: self.extra_env,
            output: RunProjectOutput {
                silent: self.run.script.silent,
                inherit_output: self.inherit_output,
                emit: self.run.script.emit,
            },
            process: RunProjectProcess {
                bail: self.bail,
                process_tracker: self.outcome.process_tracker,
                script_budget: self.script_budget,
                on_started: &on_started,
            },
        });
        self.outcome.record(node, &summary_key, execution)
    }
}

/// Where each task's result is recorded.
pub(super) struct RunOutcome<'a> {
    pub(super) result: &'a Mutex<IndexMap<String, ExecutionStatus>>,
    pub(super) has_command: &'a AtomicUsize,
    pub(super) first_failure: &'a Mutex<Option<String>>,
    pub(super) abort: &'a Mutex<Option<miette::Report>>,
    pub(super) process_tracker: Option<&'a ProcessTracker>,
    pub(super) task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
    pub(super) workspace_root: &'a Path,
}

impl RunOutcome<'_> {
    fn record(
        &self,
        node: &TaskNode,
        summary_key: &str,
        execution: miette::Result<ProjectExecution>,
    ) -> TaskCompletion {
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => return self.abort(error),
        };
        if node.requested {
            self.has_command.fetch_add(execution.has_command, Ordering::Relaxed);
        }
        let failed = execution.status.status == Status::Failure;
        let cancelled = execution.cancelled;
        let recursion_guarded = execution.recursion_guarded;
        self.result.lock().expect("summary lock is not poisoned")[summary_key] = execution.status;
        if cancelled {
            return TaskCompletion::Cancelled;
        }
        if failed {
            let mut first_failure =
                self.first_failure.lock().expect("first-failure slot lock is not poisoned");
            if first_failure.is_none() {
                *first_failure = Some(node.project.to_string_lossy().into_owned());
            }
            return TaskCompletion::Failed;
        }
        // A task the recursion guard skipped never ran, so it is not one
        // a resume may skip either.
        if recursion_guarded {
            return TaskCompletion::Passed;
        }
        let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
        match self.task_run_state.record_passed(&key, node, self.workspace_root) {
            Ok(()) => TaskCompletion::Passed,
            Err(error) => self.abort(error),
        }
    }

    /// A run that cannot record its own state aborts: continuing would
    /// build a summary nothing can act on.
    pub(super) fn abort(&self, error: miette::Report) -> TaskCompletion {
        let mut abort = self.abort.lock().expect("abort slot lock is not poisoned");
        if abort.is_none() {
            *abort = Some(error);
        }
        if let Some(process_tracker) = self.process_tracker {
            process_tracker.cancel();
        }
        TaskCompletion::Aborted
    }
}

struct ProjectExecution {
    status: ExecutionStatus,
    has_command: usize,
    cancelled: bool,
    recursion_guarded: bool,
}

#[derive(Clone, Copy)]
struct RunProjectOptions<'a, 'project> {
    node: &'a TaskNode,
    graph: &'a ProjectGraph<GraphPkg<'project>>,
    args: &'a RunArgs,
    init_cwd: &'a Path,
    config: &'a Config,
    extra_env: &'a HashMap<String, String>,
    output: RunProjectOutput,
    process: RunProjectProcess<'a>,
}

#[derive(Clone, Copy)]
pub(crate) struct RunProjectOutput {
    silent: bool,
    inherit_output: bool,
    emit: fn(&LogEvent),
}

#[derive(Clone, Copy)]
pub(crate) struct RunProjectProcess<'a> {
    bail: bool,
    process_tracker: Option<&'a ProcessTracker>,
    script_budget: &'a ScriptBudget,
    on_started: &'a (dyn Fn() + Sync),
}

/// Run the task's scripts — concurrently when the run's script-level
/// concurrency allows it, one at a time otherwise, like the
/// single-project `run` does through `run_selected_scripts`. Either way
/// each script waits for a permit from the run's [`ScriptBudget`] first.
fn run_project(options: &RunProjectOptions<'_, '_>) -> miette::Result<ProjectExecution> {
    let root = options.node.project.as_path();
    let manifest = &options.graph[root].package.project.manifest;
    let extra_env = project_extra_env(options.config, root, options.extra_env);

    // pnpm names the project directory as the `depPath` of a recursive
    // run's lifecycle events; the reporter renders `wd` and only groups
    // by this.
    let root_str = root.to_string_lossy().into_owned();
    let run = ProjectScripts {
        options,
        manifest,
        extra_env: &extra_env,
        root_str: &root_str,
        concurrency: script_concurrency(
            options.config,
            options.node.scripts.len(),
            options.args.workspace.parallel,
            options.args.sequential,
        ),
    };
    if options.node.scripts.len() > 1 && run.concurrency > 1 {
        return concurrent::run_scripts(&run);
    }
    sequential::run_scripts(&run)
}

/// One task's script run: the shared run settings plus the per-task
/// bits both execution modes need.
struct ProjectScripts<'a, 'run, 'project> {
    options: &'a RunProjectOptions<'run, 'project>,
    manifest: &'a pnpm_package_manifest::PackageManifest,
    extra_env: &'a HashMap<String, String>,
    root_str: &'a str,
    concurrency: usize,
}

/// The mutable outcome of one task's script run. A sequential run holds
/// it directly; a concurrent run guards it in a [`Mutex`] and applies
/// the same transitions. A `RegExp` selector can match several scripts in
/// one task, but the summary carries a single status per task and the
/// exit code derives from it: once one of the task's scripts has
/// failed, nothing a later one does may overwrite that — under
/// `--no-bail` the run would otherwise report itself green.
struct ScriptRunState {
    execution: ProjectExecution,
    failed: bool,
}

impl ScriptRunState {
    fn queued() -> Self {
        ScriptRunState {
            execution: ProjectExecution {
                status: ExecutionStatus::queued(),
                has_command: 0,
                cancelled: false,
                recursion_guarded: false,
            },
            failed: false,
        }
    }

    /// Book a script that is about to run.
    fn before_script(&mut self) {
        if !self.failed {
            self.execution.status.status = Status::Running;
        }
        self.execution.has_command += 1;
    }

    /// Book a script the run's cancellation ended. A task one of whose
    /// own scripts already failed keeps that verdict: the cancellation
    /// is what that failure asked for, and the summary reads the task as
    /// cancelled rather than failed otherwise.
    fn cancelled_script(&mut self) {
        if !self.failed {
            self.execution.cancelled = true;
        }
    }
}

/// Apply one settled script's verdict to the run state. `false` when
/// the run was cancelled, which ends the project outright: both the
/// tracker's own cancellation and a failure that arrives after the
/// tracker already cancelled are that cancellation, not new outcomes.
fn apply_script_result(
    state: &mut ScriptRunState,
    ctx: &RunContext<'_>,
    status: pnpm_executor::ScriptExit,
    duration: f64,
) -> bool {
    if ctx.process_tracker.is_some_and(ProcessTracker::is_cancelled) {
        state.cancelled_script();
        return false;
    }
    if status.success() {
        if !state.failed {
            record_script_pass(&mut state.execution.status, duration);
        }
        return true;
    }
    if ctx.process_tracker.is_some_and(|process_tracker| !process_tracker.cancel()) {
        state.cancelled_script();
        return false;
    }
    state.failed = true;
    record_script_failure(&mut state.execution.status, ctx.dir, status, duration);
    true
}

/// Take the run's permission to start one more script. `None` when the
/// run was cancelled while this script waited for a permit: a script
/// that only reached the front of the queue after the run gave up was
/// dispatched in name only, and starting it would grow a run that is
/// already unwinding.
fn start_script<'a>(process: &RunProjectProcess<'a>) -> Option<ScriptPermit<'a>> {
    let permit = process.script_budget.acquire();
    if process.process_tracker.is_some_and(ProcessTracker::is_cancelled) {
        return None;
    }
    Some(permit)
}

/// The script body to run for one selected name. `None` when the
/// project declares none, or when it is empty or the args-less
/// `only-allow` guard, both of which run nothing.
fn runnable_project_script(
    manifest: &pnpm_package_manifest::PackageManifest,
    selected: &str,
    args: &RunArgs,
) -> miette::Result<Option<String>> {
    let Some(script) = manifest.script(selected, true)? else {
        return Ok(None);
    };
    if script.is_empty() || (args.script_args().is_empty() && script == "npx only-allow pnpm") {
        return Ok(None);
    }
    Ok(Some(script.to_owned()))
}

/// The environment a project's scripts run under, with the resolver
/// each non-default linker needs prepended to `NODE_OPTIONS`.
fn project_extra_env(
    config: &Config,
    root: &Path,
    extra_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut extra_env = extra_env.clone();
    if let Some(pnp_path) = pnp_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_require_option(&pnp_path, node_options),
        );
    }
    if let Some(package_map_path) = package_map_path_for_execution(config, root) {
        let node_options = extra_env.get("NODE_OPTIONS").map(String::as_str);
        extra_env.insert(
            "NODE_OPTIONS".to_string(),
            make_node_package_map_option(&package_map_path, node_options),
        );
    }
    extra_env
}

/// Whether running this script would re-enter the one pnpm is already
/// running for this very project, which the inherited
/// `npm_lifecycle_event` and `PNPM_SCRIPT_SRC_DIR` identify.
fn reenters_running_script(selected: &str, root: &Path) -> bool {
    env::var_os("npm_lifecycle_event").is_some_and(|event| event == *selected)
        && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
}

/// A script's output goes straight to this process's terminal unless
/// the run interleaves several, in which case each line is prefixed with
/// its project.
fn script_output(inherit_output: bool, root_str: &str, emit: fn(&LogEvent)) -> ScriptOutput<'_> {
    if inherit_output {
        ScriptOutput::Inherit
    } else {
        ScriptOutput::Streamed { dep_path: root_str, emit }
    }
}

fn record_script_pass(status_entry: &mut ExecutionStatus, duration: f64) {
    status_entry.status = Status::Passed;
    status_entry.duration = Some(duration);
}

fn record_script_failure(
    status_entry: &mut ExecutionStatus,
    root: &Path,
    status: pnpm_executor::ScriptExit,
    duration: f64,
) {
    status_entry.status = Status::Failure;
    status_entry.duration = Some(duration);
    status_entry.message =
        Some(format!("command failed with exit code {}", status.code().unwrap_or(1)));
    status_entry.prefix = Some(root.to_string_lossy().into_owned());
}

impl RunProjectOptions<'_, '_> {
    fn run_context<'a>(
        &'a self,
        manifest: &'a pnpm_package_manifest::PackageManifest,
        root: &'a Path,
        extra_env: &'a HashMap<String, String>,
        root_str: &'a str,
    ) -> RunContext<'a> {
        RunContext {
            manifest,
            dir: root,
            init_cwd: self.init_cwd,
            config: self.config,
            extra_env,
            silent: self.output.silent,
            output: script_output(self.output.inherit_output, root_str, self.output.emit),
            process_tracker: self.process.process_tracker,
            emit: self.output.emit,
        }
    }
}

mod concurrent;

mod sequential;

#[cfg(test)]
mod tests;
