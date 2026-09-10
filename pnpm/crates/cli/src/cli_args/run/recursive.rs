//! Recursive `pacquet run` — run a package script across the
//! `--filter`-selected workspace projects, scheduled over the task graph.
//!
//! `config.filter` / `config.filter_prod` (`--filter` / `--filter-prod`,
//! include and exclude selectors) narrow the selected set via
//! [`select_recursive_projects`]; a task graph is then built over the
//! selection — the invocation's script in every project, plus what the
//! workspace's `tasks` declarations pull in — and dispatched in dependency
//! order under `workspaceConcurrency`, with no barrier between
//! dependency-independent tasks. `--no-sort` drops the ordering entirely,
//! `--reverse` runs the reverse graph, and `--parallel` starts every task
//! concurrently. The main-dispatch auto-exclusion of the workspace root is
//! applied via [`AutoExcludeRoot::Enabled`].

use super::{
    RunArgs, RunContext, ScriptSelector, get_run_script_commands, render_project_commands,
    run_stages, throw_or_filter_hidden_scripts,
};
use crate::cli_args::{
    recursive::{
        AutoExcludeRoot, ExecutionStatus, Status, count_failures, discover_workspace_projects,
        filtered_projects_dependencies, find_resume_root, select_recursive_projects,
        write_recursive_summary,
    },
    task_run_state::{TaskRunExecutionSettings, TaskRunStateContext, task_run_execution_settings},
};
use derive_more::{Display, Error};
use execution::{RunOutcome, RunSlots, TaskRunner};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_executor::{ProcessTracker, ScriptOutput};
use pnpm_package_manager::{
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, ScopeLog};
use pnpm_workspace::GraphPkg;
use pnpm_workspace_projects_graph::ProjectGraph;
use pnpm_workspace_task_scheduler::{
    BuildTaskGraphOptions, ScheduleTasksOptions, SequenceTasksOptions, TaskCompletion, TaskGraph,
    TaskKey, TaskNode, build_task_graph, is_serial_task_graph, render_task_graph_dry_run,
    resume_task_graph_from, reverse_task_graph, schedule_tasks, sequence_tasks, task_graph_to_json,
    task_summary_key,
};
use selection::{
    RunReporting, build_run_task_graph, check_a_project_has_the_script,
    filter_hidden_requested_scripts, print_run_dry_run, print_selected_project_commands,
    report_run_outcome, resume_task_graph, run_concurrency, run_process_tracker,
    run_state_settings,
};
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

/// Errors surfaced by a recursive run. The codes are the shared pnpm
/// error codes, so log consumers and `pnpm.io/errors` references stay
/// valid.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum RecursiveRunError {
    #[display("None of the packages has a \"{script_name}\" script")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT))]
    NoScript {
        #[error(not(source))]
        script_name: String,
    },

    #[display("None of the selected packages has a \"{script_name}\" script")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT))]
    NoSelectedScript {
        #[error(not(source))]
        script_name: String,
    },

    #[display("\"pnpm recursive run\" failed in {count} packages")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_FAIL))]
    RecursiveFail {
        #[error(not(source))]
        count: usize,
    },

    #[display("\"pnpm recursive run\" failed in {prefix}")]
    #[diagnostic(code(ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL))]
    RecursiveRunFirstFail {
        #[error(not(source))]
        prefix: String,
    },

    #[display("You must specify the script you want to run")]
    #[diagnostic(code(ERR_PNPM_SCRIPT_NAME_IS_REQUIRED))]
    ScriptNameRequired,
}

/// Run `args.command` across the `--filter`-selected workspace projects,
/// in task-graph dependency order. `dir` is the canonicalized working
/// directory; the workspace root (and the directory the summary is written
/// to) is `config.workspace_dir`, falling back to `dir` when no
/// `pnpm-workspace.yaml` exists.
pub fn run_recursive(
    args: &RunArgs,
    config: &Config,
    dir: &Path,
    emit: fn(&LogEvent),
    silent: bool,
) -> miette::Result<()> {
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);

    let (projects, patterns) = discover_workspace_projects(workspace_root, config)?;
    let selection = select_recursive_projects(
        &projects,
        config,
        dir,
        AutoExcludeRoot::Enabled { workspace_patterns: patterns.as_deref() },
    )?;
    let graph = &selection.selected;
    let Some(script_name) = args.script_name() else {
        return print_selected_project_commands(graph, &projects, workspace_root);
    };
    emit_selection_scope(emit, config, graph.len(), projects.len());
    // An empty `--filter` selection is a no-op (exit 0); an empty
    // workspace instead falls through to the no-script error below.
    if !projects.is_empty() && graph.is_empty() {
        return Ok(());
    }

    let run = RecursiveRun {
        args,
        config,
        dir,
        emit,
        silent,
        graph,
        selection: &selection,
        workspace_root,
        script_name,
        all_packages_selected: graph.len() == projects.len(),
    };
    run.run_and_report()
}

/// Report what the `--filter` selection resolved to before running a
/// single script, so the user can confirm it covers what they meant.
fn emit_selection_scope(emit: fn(&LogEvent), config: &Config, selected: usize, total: usize) {
    emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected,
        total: Some(total),
        workspace_prefix: config
            .workspace_dir
            .as_deref()
            .map(|dir| dir.to_string_lossy().into_owned()),
    }));
}

/// One recursive run: the selection it runs over and the settings every
/// task reads.
struct RecursiveRun<'a, 'project> {
    args: &'a RunArgs,
    config: &'a Config,
    dir: &'a Path,
    emit: fn(&LogEvent),
    silent: bool,
    graph: &'a ProjectGraph<GraphPkg<'project>>,
    selection: &'a crate::cli_args::recursive::RecursiveSelection<'project>,
    workspace_root: &'a Path,
    script_name: &'a str,
    all_packages_selected: bool,
}

/// The task graph ready to schedule, with the run-state journal started.
struct PreparedRun {
    task_graph: TaskGraph,
    sequenced_tasks: Vec<TaskKey>,
    extra_env: HashMap<String, String>,
    task_run_state: crate::cli_args::task_run_state::TaskRunState,
}

/// What the scheduled tasks left behind.
struct RunResults {
    statuses: IndexMap<String, ExecutionStatus>,
    ran_a_command: bool,
    /// The first failed project's prefix, when `--bail` stopped the run.
    first_failure: Option<String>,
}

impl RecursiveRun<'_, '_> {
    fn run_and_report(&self) -> miette::Result<()> {
        let Some(prepared) = self.prepare()? else {
            return Ok(());
        };
        let results = self.execute(&prepared)?;
        report_run_outcome(
            &RunReporting {
                args: self.args,
                script_name: self.script_name,
                workspace_root: self.workspace_root,
                all_packages_selected: self.all_packages_selected,
                ran_a_command: results.ran_a_command,
                task_run_state: &prepared.task_run_state,
            },
            &results.statuses,
            results.first_failure,
        )
    }

    /// Build, resume and sequence the task graph, then start the run-state
    /// journal. `None` once a dry run has printed the graph instead.
    fn prepare(&self) -> miette::Result<Option<PreparedRun>> {
        // Compiled once for the whole run, not per project or task.
        let full_task_graph = self.task_graph()?;
        let extra_env: HashMap<String, String> = self.config.extra_env_with_node_options();
        let state_settings = run_state_settings(self.config, &extra_env);
        let task_run_state_context = TaskRunStateContext::new(
            "run",
            &self.args.script,
            &state_settings,
            &full_task_graph,
            self.workspace_root,
            |node, script| self.script_commands(node, script),
        );
        let mut task_graph = resume_task_graph(
            &task_run_state_context,
            self.args,
            self.graph,
            &full_task_graph,
            self.script_name,
        )?;
        // Also the cycle check: a cyclic graph cannot be scheduled, and
        // sequenced into an arbitrary order it would succeed or fail by luck.
        let sequenced_tasks = sequence_tasks(
            &mut task_graph,
            &SequenceTasksOptions {
                workspace_dir: self.workspace_root,
                ignore_cycles: self.config.ignore_workspace_cycles,
                emit: self.emit,
            },
        )?;

        if self.args.dry_run {
            print_run_dry_run(self.args, &task_graph, &sequenced_tasks, self.workspace_root)?;
            return Ok(None);
        }

        // Hidden scripts (names starting with `.`) can only be invoked from
        // within another script, detected by an inherited
        // `npm_lifecycle_event`. Checked only for the tasks the invocation
        // named: a `dependsOn` declaration naming a hidden script is a
        // deliberate reference, like a call from another script.
        filter_hidden_requested_scripts(&mut task_graph, self.script_name)?;

        check_a_project_has_the_script(
            &task_graph,
            self.args,
            self.script_name,
            self.all_packages_selected,
        )?;

        let task_run_state = task_run_state_context
            .start(&initially_completed_tasks(&full_task_graph, &task_graph))?;
        Ok(Some(PreparedRun { task_graph, sequenced_tasks, extra_env, task_run_state }))
    }

    fn task_graph(&self) -> miette::Result<TaskGraph> {
        let selector = ScriptSelector::new(self.script_name)?;
        build_run_task_graph(
            self.script_name,
            &selector,
            self.args,
            self.config,
            self.graph,
            self.selection,
            self.emit,
        )
    }

    fn script_commands(&self, node: &TaskNode, script: &str) -> Vec<String> {
        let manifest = &self.graph[&node.project].package.project.manifest;
        let Some(main) =
            manifest.script(script, true).expect("if-present script lookup cannot fail")
        else {
            return Vec::new();
        };
        get_run_script_commands(manifest, script, main, self.config.enable_pre_post_scripts)
    }

    /// Schedule every task and collect what each left behind.
    fn execute(&self, prepared: &PreparedRun) -> miette::Result<RunResults> {
        let bail = !self.args.no_bail;
        let concurrency = run_concurrency(self.args, self.config, prepared.task_graph.len());
        let runs_concurrently = concurrency > 1
            && !is_serial_task_graph(&prepared.task_graph, &prepared.sequenced_tasks);
        let slots = RunSlots::queued(&prepared.task_graph);
        let process_tracker = run_process_tracker(bail, runs_concurrently);
        let init_cwd = env::current_dir().unwrap_or_else(|_| self.dir.to_path_buf());
        let runner = TaskRunner {
            run: self,
            outcome: RunOutcome {
                result: &slots.result,
                has_command: &slots.has_command,
                first_failure: &slots.first_failure,
                abort: &slots.abort,
                process_tracker: process_tracker.as_ref(),
                task_run_state: &prepared.task_run_state,
                workspace_root: self.workspace_root,
            },
            extra_env: &prepared.extra_env,
            init_cwd: &init_cwd,
            bail,
            inherit_output: !self.config.stream && !runs_concurrently,
        };
        let run_task = |node: &TaskNode| runner.run_task(node);
        let on_task_skipped = |node: &TaskNode| {
            slots.result.lock().expect("summary lock is not poisoned")[&task_summary_key(node)]
                .status = Status::Skipped;
        };
        schedule_tasks(
            &prepared.task_graph,
            &ScheduleTasksOptions {
                concurrency,
                bail,
                run_task: &run_task,
                on_task_skipped: &on_task_skipped,
            },
        );
        slots.into_results(bail)
    }
}

/// The tasks a resumed run already completed: those the full graph has
/// and the resumed graph does not.
fn initially_completed_tasks(
    full_task_graph: &TaskGraph,
    task_graph: &TaskGraph,
) -> HashSet<TaskKey> {
    full_task_graph.keys().filter(|key| !task_graph.contains_key(*key)).cloned().collect()
}

mod execution;

mod selection;
