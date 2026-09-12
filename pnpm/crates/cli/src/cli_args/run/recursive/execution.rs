use super::{
    AtomicUsize, Config, ExecutionStatus, GraphPkg, HashMap, IndexMap, Instant, LogEvent, Mutex,
    Ordering, Path, ProcessTracker, ProjectGraph, RecursiveRun, RunArgs, RunContext, RunResults,
    ScriptOutput, Status, TaskCompletion, TaskGraph, TaskKey, TaskNode, env,
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution, run_stages, task_summary_key,
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
    pub(super) init_cwd: &'a Path,
    pub(super) bail: bool,
    /// pnpm pipes unless the output cannot interleave: `--stream` off, and
    /// the graph cannot put two scripts in flight at once.
    pub(super) inherit_output: bool,
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
            bail: self.bail,
            silent: self.run.silent,
            inherit_output: self.inherit_output,
            emit: self.run.emit,
            process_tracker: self.outcome.process_tracker,
            on_started: &on_started,
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
    bail: bool,
    silent: bool,
    inherit_output: bool,
    emit: fn(&LogEvent),
    process_tracker: Option<&'a ProcessTracker>,
    on_started: &'a dyn Fn(),
}

fn run_project(options: &RunProjectOptions<'_, '_>) -> miette::Result<ProjectExecution> {
    let root = options.node.project.as_path();
    let manifest = &options.graph[root].package.project.manifest;
    let extra_env = project_extra_env(options.config, root, options.extra_env);

    // pnpm names the project directory as the `depPath` of a recursive
    // run's lifecycle events; the reporter renders `wd` and only groups
    // by this.
    let root_str = root.to_string_lossy().into_owned();
    let mut execution = ProjectExecution {
        status: ExecutionStatus::queued(),
        has_command: 0,
        cancelled: false,
        recursion_guarded: false,
    };
    let mut project_failed = false;
    for selected in &options.node.scripts {
        let Some(script) = runnable_project_script(manifest, selected, options.args)? else {
            continue;
        };
        // Running the script pnpm is already inside would recurse; the
        // guard is what `pnpm -r test` from within a `test` script needs.
        if reenters_running_script(selected, root) {
            execution.recursion_guarded = true;
            continue;
        }

        (options.on_started)();
        if !project_failed {
            execution.status.status = Status::Running;
        }
        execution.has_command += 1;
        let ctx = options.run_context(manifest, root, &extra_env, &root_str);
        let ran = run_one_project_script(
            &ctx,
            selected,
            &script,
            options.args,
            &mut execution,
            &mut project_failed,
        )?;
        // A cancelled run ends the project outright; `--bail` stops it
        // at the first script that failed.
        if !ran || (project_failed && options.bail) {
            break;
        }
    }
    Ok(execution)
}

/// Run one of a project's scripts and record its verdict. `Ok(false)`
/// when the run was cancelled, which ends the project outright.
fn run_one_project_script(
    ctx: &RunContext<'_>,
    selected: &str,
    script: &str,
    args: &RunArgs,
    execution: &mut ProjectExecution,
    project_failed: &mut bool,
) -> miette::Result<bool> {
    let process_tracker = ctx.process_tracker;
    let start = Instant::now();
    let status = run_stages(ctx, selected, script, args.script_args())?;
    let duration = start.elapsed().as_secs_f64() * 1e3;

    if process_tracker.is_some_and(ProcessTracker::is_cancelled) {
        execution.cancelled = true;
        return Ok(false);
    }
    if status.success() {
        // A project that already failed keeps that verdict, whatever its
        // later scripts do.
        if !*project_failed {
            record_script_pass(&mut execution.status, duration);
        }
        return Ok(true);
    }
    // A failure after the tracker already cancelled is that cancellation,
    // not a new one.
    if process_tracker.is_some_and(|process_tracker| !process_tracker.cancel()) {
        execution.cancelled = true;
        return Ok(false);
    }
    *project_failed = true;
    record_script_failure(&mut execution.status, ctx.dir, status, duration);
    Ok(true)
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
        extra_env
            .insert("NODE_OPTIONS".to_string(), make_node_require_option(&pnp_path, node_options));
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
            silent: self.silent,
            output: script_output(self.inherit_output, root_str, self.emit),
            process_tracker: self.process_tracker,
        }
    }
}
