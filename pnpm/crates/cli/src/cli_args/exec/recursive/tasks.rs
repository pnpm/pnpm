use super::{
    Config, ExecArgs, ExecError, ExecutionStatus, IndexMap, Instant, LogEvent, Mutex, Path,
    PathBuf, ProcessTracker, ProjectExecution, RecursiveExecError, ScriptOutput, Status,
    TaskCompletion, TaskGraph, TaskKey, TaskNode, count_failures, filtered_projects_dependencies,
    reverse_task_graph, spawn_exec_task, write_recursive_summary,
};

/// Everything one project's exec run reads and records.
pub(super) struct ExecTaskContext<'a> {
    pub(super) args: &'a ExecArgs,
    pub(super) config: &'a Config,
    pub(super) command: &'a [String],
    pub(super) dir: &'a Path,
    pub(super) workspace_root: &'a Path,
    pub(super) show_prefix: bool,
    pub(super) emit: fn(&LogEvent),
    pub(super) result: &'a Mutex<IndexMap<String, ExecutionStatus>>,
    pub(super) first_failure: &'a Mutex<Option<String>>,
    pub(super) abort: &'a Mutex<Option<miette::Report>>,
    pub(super) process_tracker: Option<&'a ProcessTracker>,
    pub(super) task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
}

pub(super) fn run_exec_task(context: &ExecTaskContext<'_>, node: &TaskNode) -> TaskCompletion {
    let root = node.project.as_path();
    let prefix = root.to_string_lossy().into_owned();
    context.result.lock().expect("summary lock is not poisoned")[&prefix].status = Status::Running;
    let start = Instant::now();
    let outcome = spawn_exec_task(context, root);
    let execution = project_execution(start, outcome);
    let mut result = context.result.lock().expect("summary lock is not poisoned");
    let entry = &mut result[&prefix];
    if context.process_tracker.is_some_and(ProcessTracker::is_cancelled)
        && execution.message.is_none()
    {
        return TaskCompletion::Cancelled;
    }
    entry.duration = Some(execution.duration);
    let Some(message) = execution.message else {
        entry.status = Status::Passed;
        drop(result);
        return record_task_passed(
            context.task_run_state,
            context.abort,
            context.process_tracker,
            node,
            context.workspace_root,
        );
    };
    // A failure that follows the tracker's own cancellation is that
    // cancellation, not a new one.
    if context.process_tracker.is_some_and(|tracker| !tracker.cancel()) {
        return TaskCompletion::Cancelled;
    }
    entry.status = Status::Failure;
    entry.message = Some(message);
    entry.prefix = Some(prefix.clone());
    drop(result);
    record_first_failure(context.first_failure, prefix)
}

/// One task per selected project, wired in dependency order when
/// `--sort` asks for it.
pub(super) fn build_exec_task_graph(
    args: &ExecArgs,
    graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    command_name: &str,
) -> TaskGraph {
    let project_dependencies: IndexMap<PathBuf, Vec<PathBuf>> = if args.sort {
        filtered_projects_dependencies(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        )
    } else {
        graph.keys().cloned().map(|root| (root, Vec::new())).collect()
    };
    let task_graph: TaskGraph = project_dependencies
        .iter()
        .map(|(project, dependencies)| {
            let key = TaskKey { project: project.clone(), task_name: command_name.to_owned() };
            let node = TaskNode {
                project: project.clone(),
                task_name: command_name.to_owned(),
                concurrency: None,
                scripts: vec![command_name.to_owned()],
                requested: true,
                dependencies: dependencies
                    .iter()
                    .map(|dependency| TaskKey {
                        project: dependency.clone(),
                        task_name: command_name.to_owned(),
                    })
                    .collect(),
            };
            (key, node)
        })
        .collect();
    if args.reverse { reverse_task_graph(&task_graph) } else { task_graph }
}

/// `--parallel` runs every task at once; otherwise the workspace
/// concurrency setting caps it, never below one.
pub(super) fn exec_concurrency(args: &ExecArgs, config: &Config, task_count: usize) -> usize {
    if args.parallel {
        return task_count;
    }
    usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
}

/// Record a passed task in the run state, aborting the whole run when
/// the state cannot be written — a run whose state is unrecorded cannot
/// be resumed, so continuing would build a summary nothing can act on.
fn record_task_passed(
    task_run_state: &crate::cli_args::task_run_state::TaskRunState,
    abort: &Mutex<Option<miette::Report>>,
    process_tracker: Option<&ProcessTracker>,
    node: &TaskNode,
    workspace_root: &Path,
) -> TaskCompletion {
    let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
    let Err(error) = task_run_state.record_passed(&key, node, workspace_root) else {
        return TaskCompletion::Passed;
    };
    let mut abort = abort.lock().expect("abort slot lock is not poisoned");
    if abort.is_none() {
        *abort = Some(error);
    }
    if let Some(process_tracker) = process_tracker {
        process_tracker.cancel();
    }
    TaskCompletion::Aborted
}

/// The first failing project's prefix is the one the error names.
fn record_first_failure(first_failure: &Mutex<Option<String>>, prefix: String) -> TaskCompletion {
    let mut first_failure = first_failure.lock().expect("first-failure slot lock is not poisoned");
    if first_failure.is_none() {
        *first_failure = Some(prefix);
    }
    TaskCompletion::Failed
}

/// Write the summary the run asked for and turn the collected statuses
/// into the command's exit status.
pub(super) fn report_recursive_outcome(
    args: &ExecArgs,
    workspace_root: &Path,
    result: &IndexMap<String, ExecutionStatus>,
    bail_prefix: Option<String>,
) -> miette::Result<()> {
    if args.report_summary {
        write_recursive_summary(workspace_root, result)?;
    }
    if let Some(prefix) = bail_prefix {
        return Err(RecursiveExecError::RecursiveExecFirstFail { prefix }.into());
    }
    let failures = count_failures(result);
    if failures > 0 {
        return Err(RecursiveExecError::RecursiveFail { count: failures }.into());
    }
    Ok(())
}

pub(super) fn project_dep_path(root: &Path, dir: &Path, show_prefix: bool) -> Option<String> {
    show_prefix.then(|| {
        pnpm_workspace::read_project_name(root).unwrap_or_else(|| {
            pathdiff::diff_paths(root, dir)
                .unwrap_or_else(|| root.to_path_buf())
                .to_string_lossy()
                .into_owned()
        })
    })
}

pub(super) fn project_output(dep_path: Option<&str>, emit: fn(&LogEvent)) -> ScriptOutput<'_> {
    match dep_path {
        Some(dep_path) => ScriptOutput::Streamed { dep_path, emit },
        None => ScriptOutput::Inherit,
    }
}

fn project_execution(
    start: Instant,
    outcome: Result<std::process::ExitStatus, ExecError>,
) -> ProjectExecution {
    let duration = start.elapsed().as_secs_f64() * 1e3;
    let message = match outcome {
        Ok(status) if status.success() => None,
        Ok(status) => Some(format!("command failed with exit code {}", status.code().unwrap_or(1))),
        Err(error) => Some(error.to_string()),
    };
    ProjectExecution { duration, message }
}
