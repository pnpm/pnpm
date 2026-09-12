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
use tasks::{
    ExecTaskContext, build_exec_task_graph, exec_concurrency, project_dep_path, project_output,
    report_recursive_outcome, run_exec_task,
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
    // An empty `--filter` selection is a no-op (exit 0).
    if selection.selected.is_empty() {
        return Ok(());
    }

    execute_selection(args, config, dir, emit, &command, workspace_root, &selection)
}

/// What identifies an exec run in the task-run state: its command line and
/// the execution settings it ran under.
struct ExecStateInputs {
    params: Vec<String>,
    settings: Vec<String>,
}

impl ExecStateInputs {
    fn new(args: &ExecArgs, config: &Config) -> Self {
        let mut params = args.command.clone();
        params.push(format!("shell-mode={}", args.shell_mode));
        let extra_env = config.extra_env_with_node_options();
        let settings = task_run_execution_settings(&TaskRunExecutionSettings {
            extra_bin_paths: &config.extra_bin_paths,
            extra_env: &extra_env,
            modules_dir: &config.modules_dir,
            node_experimental_package_map: config.node_experimental_package_map,
            node_options: config.node_options.as_deref(),
            user_agent: &config.user_agent,
        });
        Self { params, settings }
    }
}

/// The task graph to run: the full graph, or the part after `--resume-from`
/// with the tasks a previous run already completed taken out.
fn resumed_exec_task_graph(
    args: &ExecArgs,
    graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
    full_task_graph: &TaskGraph,
    task_run_state_context: &TaskRunStateContext,
) -> miette::Result<TaskGraph> {
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
    Ok(match resume_anchor {
        Some(anchor) => resume_task_graph_from(
            full_task_graph.clone(),
            &anchor,
            &args.command[0],
            completed_tasks.as_ref(),
        ),
        None => full_task_graph.clone(),
    })
}

/// The tasks a resumed run starts with already completed.
fn initially_completed(full_task_graph: &TaskGraph, task_graph: &TaskGraph) -> HashSet<TaskKey> {
    full_task_graph.keys().filter(|key| !task_graph.contains_key(*key)).cloned().collect()
}

/// One recursive exec, ready to be scheduled over its task graph.
struct ExecRun<'a> {
    args: &'a ExecArgs,
    config: &'a Config,
    command: &'a [String],
    dir: &'a Path,
    workspace_root: &'a Path,
    emit: fn(&LogEvent),
    task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
}

impl ExecRun<'_> {
    /// Run every task, then report the outcome the way the flags ask for.
    fn execute(&self, task_graph: &TaskGraph, sequenced_tasks: &[TaskKey]) -> miette::Result<()> {
        let bail = !self.args.no_bail;
        let concurrency = exec_concurrency(self.args, self.config, task_graph.len());
        let result = queued_exec_results(task_graph);
        let first_failure: Mutex<Option<String>> = Mutex::new(None);
        let abort: Mutex<Option<miette::Report>> = Mutex::new(None);
        let runs_concurrently =
            concurrency > 1 && !is_serial_task_graph(task_graph, sequenced_tasks);
        let process_tracker = exec_process_tracker(bail, runs_concurrently);
        let task_context = ExecTaskContext {
            args: self.args,
            config: self.config,
            command: self.command,
            dir: self.dir,
            workspace_root: self.workspace_root,
            // Unlike `run`'s `--stream`, `exec` prefixes its output only when
            // the user turned the hiding off explicitly — pnpm gates on
            // `reporterHidePrefix === false`, not on its falsiness.
            show_prefix: self.config.reporter_hide_prefix == Some(false),
            emit: self.emit,
            result: &result,
            first_failure: &first_failure,
            abort: &abort,
            process_tracker: process_tracker.as_ref(),
            task_run_state: self.task_run_state,
        };
        schedule_exec_tasks(&task_context, task_graph, concurrency, bail);

        if let Some(error) = abort.into_inner().expect("abort slot lock is not poisoned") {
            return Err(error);
        }

        let result = result.into_inner().expect("summary lock is not poisoned");
        let first_failure =
            first_failure.into_inner().expect("first-failure slot lock is not poisoned");
        report_recursive_outcome(
            self.args,
            self.workspace_root,
            &result,
            bail.then_some(first_failure).flatten(),
        )
    }
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

fn execute_selection(
    args: &ExecArgs,
    config: &Config,
    dir: &Path,
    emit: fn(&LogEvent),
    command: &[String],
    workspace_root: &Path,
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
) -> miette::Result<()> {
    let full_task_graph =
        build_exec_task_graph(args, &selection.selected, selection, &args.command[0]);
    let state_inputs = ExecStateInputs::new(args, config);
    let task_run_state_context = TaskRunStateContext::new(
        "exec",
        &state_inputs.params,
        &state_inputs.settings,
        &full_task_graph,
        workspace_root,
        |_, _| Vec::new(),
    );
    let mut task_graph = resumed_exec_task_graph(
        args,
        &selection.selected,
        &full_task_graph,
        &task_run_state_context,
    )?;
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

    let task_run_state =
        task_run_state_context.start(&initially_completed(&full_task_graph, &task_graph))?;
    let run = ExecRun {
        args,
        config,
        command,
        dir,
        workspace_root,
        emit,
        task_run_state: &task_run_state,
    };
    run.execute(&task_graph, &sequenced_tasks)?;
    task_run_state.finish()?;
    Ok(())
}

fn schedule_exec_tasks(
    context: &ExecTaskContext<'_>,
    task_graph: &TaskGraph,
    concurrency: usize,
    bail: bool,
) {
    let run_task = |node: &TaskNode| run_exec_task(context, node);
    let on_task_skipped = |node: &TaskNode| {
        context.result.lock().expect("summary lock is not poisoned")
            [&node.project.to_string_lossy().into_owned()]
            .status = Status::Skipped;
    };
    schedule_tasks(
        task_graph,
        &ScheduleTasksOptions {
            concurrency,
            bail,
            run_task: &run_task,
            on_task_skipped: &on_task_skipped,
        },
    );
}

fn queued_exec_results(task_graph: &TaskGraph) -> Mutex<IndexMap<String, ExecutionStatus>> {
    Mutex::new(
        task_graph
            .values()
            .map(|node| (node.project.to_string_lossy().into_owned(), ExecutionStatus::queued()))
            .collect(),
    )
}

fn spawn_exec_task(
    context: &ExecTaskContext<'_>,
    root: &Path,
) -> Result<std::process::ExitStatus, ExecError> {
    let dep_path = project_dep_path(root, context.dir, context.show_prefix);
    let output = project_output(dep_path.as_deref(), context.emit);
    spawn_in_dir(
        context.command,
        ExecDirs::same(root),
        context.config,
        context.args.shell_mode,
        output,
        context.process_tracker,
    )
}

mod tasks;
