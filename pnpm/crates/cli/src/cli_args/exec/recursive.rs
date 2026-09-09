//! Recursive `pacquet exec` — run a command across the `--filter`-selected
//! workspace projects, scheduled over the project dependency graph.
//!
//! Reuses the shared graph / summary machinery in
//! [`crate::cli_args::recursive`] and the workspace task scheduler.
//!
//! `exec` runs one command per project, so its task graph is one task per
//! selected project over the project dependency edges: it gets the
//! dependency-order scheduling, while `tasks` declarations — which name
//! scripts — do not apply to it. `--no-sort` drops the ordering,
//! `--reverse` runs the reverse graph, and `--parallel` starts every
//! project concurrently.

use super::{ExecArgs, ExecDirs, ExecError, prepare_command, spawn_in_dir};
use crate::cli_args::{
    recursive::{
        AutoExcludeRoot, ExecutionStatus, Status, count_failures, discover_workspace_projects,
        filtered_projects_dependencies, find_resume_root, select_recursive_projects,
        write_recursive_summary,
    },
    task_run_state::{TaskRunExecutionSettings, TaskRunStateContext, task_run_execution_settings},
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_executor::{ProcessTracker, ScriptOutput};
use pnpm_reporter::LogEvent;
use pnpm_workspace_task_scheduler::{
    ScheduleTasksOptions, SequenceTasksOptions, TaskCompletion, TaskGraph, TaskKey, TaskNode,
    is_serial_task_graph, resume_task_graph_from, reverse_task_graph, schedule_tasks,
    sequence_tasks,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

/// Errors surfaced by a recursive exec. Codes mirror pnpm's so log
/// consumers and `pnpm.io/errors` references stay valid across the two
/// implementations.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RecursiveExecError {
    #[display("No package found in this workspace")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_NO_PACKAGE))]
    NoPackage,

    #[display("\"pnpm recursive exec\" failed in {count} packages")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_FAIL))]
    RecursiveFail {
        #[error(not(source))]
        count: usize,
    },

    #[display("\"pnpm recursive exec\" failed in {prefix}")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL))]
    RecursiveExecFirstFail {
        #[error(not(source))]
        prefix: String,
    },
}

struct ProjectExecution {
    duration: f64,
    message: Option<String>,
}

/// Run `args.command` across the `--filter`-selected workspace projects,
/// in dependency order. `dir` is the canonicalized working directory; the
/// workspace root (and the directory the summary is written to) is
/// `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub async fn exec_recursive(
    args: &ExecArgs,
    config: &Config,
    dir: &Path,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    let command = prepare_command(args.command.clone())?;
    // Unlike `run`'s `--stream`, `exec` prefixes its output only when
    // the user turned the hiding off explicitly — pnpm gates on
    // `reporterHidePrefix === false`, not on its falsiness.
    let show_prefix = config.reporter_hide_prefix == Some(false);
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let (projects, patterns) = discover_workspace_projects(workspace_root, config)?;
    // Empty workspace errors; an empty `--filter` selection (below) is a
    // no-op — so this guard is on the discovered set, not the filtered.
    if projects.is_empty() {
        return Err(RecursiveExecError::NoPackage.into());
    }

    let selection = select_recursive_projects(
        &projects,
        config,
        dir,
        AutoExcludeRoot::Enabled { workspace_patterns: patterns.as_deref() },
    )?;
    let graph = &selection.selected;
    // An empty `--filter` selection is a no-op (exit 0).
    if graph.is_empty() {
        return Ok(());
    }

    let command_name = &args.command[0];
    let task_graph = build_exec_task_graph(args, graph, &selection, command_name);
    let full_task_graph = task_graph;
    let mut state_params = args.command.clone();
    state_params.push(format!("shell-mode={}", args.shell_mode));
    let state_extra_env = config.extra_env_with_node_options();
    let state_settings = task_run_execution_settings(&TaskRunExecutionSettings {
        extra_bin_paths: &config.extra_bin_paths,
        extra_env: &state_extra_env,
        modules_dir: &config.modules_dir,
        node_experimental_package_map: config.node_experimental_package_map,
        node_options: config.node_options.as_deref(),
        user_agent: &config.user_agent,
    });
    let task_run_state_context = TaskRunStateContext::new(
        "exec",
        &state_params,
        &state_settings,
        &full_task_graph,
        workspace_root,
        |_, _| Vec::new(),
    );
    let resume_anchor = args
        .resume_from
        .as_ref()
        .map(|resume_from| find_resume_root(resume_from, graph))
        .transpose()?;
    let completed_tasks = resume_anchor
        .as_ref()
        .map(|_| task_run_state_context.read_completed_tasks())
        .transpose()?
        .flatten();
    let mut task_graph = match resume_anchor {
        Some(anchor) => resume_task_graph_from(
            full_task_graph.clone(),
            &anchor,
            command_name,
            completed_tasks.as_ref(),
        ),
        None => full_task_graph.clone(),
    };
    // Also the cycle check: a cyclic graph cannot be scheduled, and
    // sequenced into an arbitrary order it would succeed or fail by luck.
    let sequenced_tasks = sequence_tasks(
        &mut task_graph,
        &SequenceTasksOptions {
            workspace_dir: workspace_root,
            ignore_cycles: config.ignore_workspace_cycles,
            emit,
        },
    )?;

    let initially_completed: HashSet<TaskKey> =
        full_task_graph.keys().filter(|key| !task_graph.contains_key(*key)).cloned().collect();
    let task_run_state = task_run_state_context.start(&initially_completed)?;

    let bail = !args.no_bail;
    let concurrency = exec_concurrency(args, config, task_graph.len());
    let result: Mutex<IndexMap<String, ExecutionStatus>> = Mutex::new(
        task_graph
            .values()
            .map(|node| (node.project.to_string_lossy().into_owned(), ExecutionStatus::queued()))
            .collect(),
    );
    let first_failure: Mutex<Option<String>> = Mutex::new(None);
    let abort: Mutex<Option<miette::Report>> = Mutex::new(None);
    let runs_concurrently = concurrency > 1 && !is_serial_task_graph(&task_graph, &sequenced_tasks);
    let process_tracker = exec_process_tracker(bail, runs_concurrently);
    let task_context = ExecTaskContext {
        args,
        config,
        command: &command,
        dir,
        workspace_root,
        show_prefix,
        emit,
        result: &result,
        first_failure: &first_failure,
        abort: &abort,
        process_tracker: process_tracker.as_ref(),
        task_run_state: &task_run_state,
    };
    let run_task = |node: &TaskNode| run_exec_task(&task_context, node);
    let on_task_skipped = |node: &TaskNode| {
        result.lock().expect("summary lock is not poisoned")
            [&node.project.to_string_lossy().into_owned()]
            .status = Status::Skipped;
    };
    schedule_tasks(
        &task_graph,
        &ScheduleTasksOptions {
            concurrency,
            bail,
            run_task: &run_task,
            on_task_skipped: &on_task_skipped,
        },
    );

    if let Some(error) = abort.into_inner().expect("abort slot lock is not poisoned") {
        return Err(error);
    }

    let result = result.into_inner().expect("summary lock is not poisoned");
    let first_failure =
        first_failure.into_inner().expect("first-failure slot lock is not poisoned");
    report_recursive_outcome(
        args,
        workspace_root,
        &result,
        bail.then_some(first_failure).flatten(),
    )?;
    task_run_state.finish()?;
    Ok(())
}

/// `--no-bail` runs every task whatever fails, so it needs no tracker at
/// all. A serial graph runs its child in the foreground, where Ctrl-C
/// reaches it through the terminal instead.
fn exec_process_tracker(bail: bool, runs_concurrently: bool) -> Option<ProcessTracker> {
    if !bail {
        return None;
    }
    Some(if runs_concurrently { ProcessTracker::default() } else { ProcessTracker::foreground() })
}

/// Everything one project's exec run reads and records.
struct ExecTaskContext<'a> {
    args: &'a ExecArgs,
    config: &'a Config,
    command: &'a [String],
    dir: &'a Path,
    workspace_root: &'a Path,
    show_prefix: bool,
    emit: fn(&LogEvent),
    result: &'a Mutex<IndexMap<String, ExecutionStatus>>,
    first_failure: &'a Mutex<Option<String>>,
    abort: &'a Mutex<Option<miette::Report>>,
    process_tracker: Option<&'a ProcessTracker>,
    task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
}

fn run_exec_task(context: &ExecTaskContext<'_>, node: &TaskNode) -> TaskCompletion {
    let root = node.project.as_path();
    let prefix = root.to_string_lossy().into_owned();
    context.result.lock().expect("summary lock is not poisoned")[&prefix].status = Status::Running;
    let start = Instant::now();
    let dep_path = project_dep_path(root, context.dir, context.show_prefix);
    let output = project_output(dep_path.as_deref(), context.emit);
    let outcome = spawn_in_dir(
        context.command,
        ExecDirs::same(root),
        context.config,
        context.args.shell_mode,
        output,
        context.process_tracker,
    );
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
fn build_exec_task_graph(
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
fn exec_concurrency(args: &ExecArgs, config: &Config, task_count: usize) -> usize {
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
fn report_recursive_outcome(
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

fn project_dep_path(root: &Path, dir: &Path, show_prefix: bool) -> Option<String> {
    show_prefix.then(|| {
        pnpm_workspace::read_project_name(root).unwrap_or_else(|| {
            pathdiff::diff_paths(root, dir)
                .unwrap_or_else(|| root.to_path_buf())
                .to_string_lossy()
                .into_owned()
        })
    })
}

fn project_output(dep_path: Option<&str>, emit: fn(&LogEvent)) -> ScriptOutput<'_> {
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
