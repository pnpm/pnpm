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
//! concurrently. A task whose selector matched several scripts runs them
//! side by side rather than one after another, and every script of the
//! run draws on one [`ScriptBudget`], so `workspaceConcurrency` bounds
//! the processes a run without `--parallel` has running, not the tasks
//! it dispatched. The main-dispatch auto-exclusion of the workspace root
//! is applied via [`AutoExcludeRoot::Enabled`].

use super::{
    RunArgs, RunContext, ScriptSelector, get_run_script_commands, render_project_commands,
    run_stages, script_concurrency, throw_or_filter_hidden_scripts,
};
use crate::cli_args::{
    recursive::{
        AutoExcludeRoot, ExecutionStatus, Status, count_failures, discover_workspace_projects,
        filtered_projects_dependencies, find_resume_root, no_projects_matched_message,
        notice_workspace_dir, select_recursive_projects, write_recursive_summary,
    },
    reporter::{ReporterType, reporter_emit},
    task_run_state::{TaskRunExecutionSettings, TaskRunStateContext, task_run_execution_settings},
    verify_deps::verify_deps_before_recursive_run,
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
use script_budget::{ScriptBudget, ScriptPermit, run_script_budget};
use selection::{
    RunReporting, a_project_has_the_script, build_run_task_graph, check_a_project_has_the_script,
    filter_hidden_requested_scripts, print_run_dry_run, print_selected_project_commands,
    report_run_outcome, resume_task_graph, run_concurrency, run_process_tracker,
    run_state_settings,
};
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex,
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

pub enum RecursiveRunOutcome {
    Done,
    /// No selected project has a script the name matches, and the caller
    /// asked to fall back to `exec` instead of failing. Returned before
    /// anything is dispatched.
    NoMatchingScript,
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
    reporter: ReporterType,
    fallback_to_exec: bool,
) -> miette::Result<RecursiveRunOutcome> {
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
        print_selected_project_commands(graph, &projects, workspace_root)?;
        return Ok(RecursiveRunOutcome::Done);
    };
    let emit = reporter_emit(reporter);
    let silent = matches!(reporter, ReporterType::Ndjson | ReporterType::Silent);
    emit_selection_scope(emit, config, graph.len(), projects.len());
    // An empty `--filter` selection is a no-op (exit 0); an empty
    // workspace instead falls through to the no-script error below.
    if !projects.is_empty() && graph.is_empty() {
        if !args.json {
            println!("{}", no_projects_matched_message(notice_workspace_dir(config, dir)));
        }
        return Ok(RecursiveRunOutcome::Done);
    }

    let run = RecursiveRun {
        args,
        config,
        dir,
        graph,
        reporter,
        selection: &selection,
        workspace_root,
        script: RecursiveScript {
            emit,
            silent,
            script_name,
            all_packages_selected: graph.len() == projects.len(),
            fallback_to_exec,
        },
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
        workspace_prefix: config.workspace_dir
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
    graph: &'a ProjectGraph<GraphPkg<'project>>,
    reporter: ReporterType,
    selection: &'a crate::cli_args::recursive::RecursiveSelection<'project>,
    workspace_root: &'a Path,
    script: RecursiveScript<'a>,
}

pub(crate) struct RecursiveScript<'a> {
    emit: fn(&LogEvent),
    silent: bool,
    script_name: &'a str,
    all_packages_selected: bool,
    /// Whether a name no selected project has a script for is handed to
    /// `exec`, as the `pnpm <command>` shorthand does.
    fallback_to_exec: bool,
}

enum Prepared {
    Run(Box<PreparedRun>),
    DryRun,
    NoMatchingScript,
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
    fn run_and_report(&self) -> miette::Result<RecursiveRunOutcome> {
        let prepared = match self.prepare()? {
            Prepared::Run(prepared) => *prepared,
            Prepared::DryRun => return Ok(RecursiveRunOutcome::Done),
            Prepared::NoMatchingScript => return Ok(RecursiveRunOutcome::NoMatchingScript),
        };
        let projects_to_verify = prepared.task_graph
            .values()
            .filter(|node| !node.scripts.is_empty())
            .map(|node| node.project.as_path());
        verify_deps_before_recursive_run(
            self.workspace_root,
            projects_to_verify,
            self.config,
            self.reporter,
        )?;
        let results = self.execute(&prepared)?;
        report_run_outcome(
            &RunReporting {
                args: self.args,
                script_name: self.script.script_name,
                workspace_root: self.workspace_root,
                all_packages_selected: self.script.all_packages_selected,
                ran_a_command: results.ran_a_command,
                task_run_state: &prepared.task_run_state,
            },
            &results.statuses,
            results.first_failure,
        )?;
        Ok(RecursiveRunOutcome::Done)
    }

    /// Build, resume and sequence the task graph, then start the run-state
    /// journal.
    fn prepare(&self) -> miette::Result<Prepared> {
        // Compiled once for the whole run, not per project or task.
        let full_task_graph = self.task_graph()?;
        if self.falls_back_to_exec(&full_task_graph) {
            return Ok(Prepared::NoMatchingScript);
        }
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
            self.script.script_name,
        )?;
        let sequenced_tasks = self.sequence(&mut task_graph)?;

        if self.args.dry_run {
            print_run_dry_run(self.args, &task_graph, &sequenced_tasks, self.workspace_root)?;
            return Ok(Prepared::DryRun);
        }

        self.validate_requested_scripts(&mut task_graph)?;

        let task_run_state = task_run_state_context.start(&initially_completed_tasks(
            &full_task_graph,
            &task_graph,
        ))?;
        Ok(Prepared::Run(Box::new(PreparedRun {
            task_graph,
            sequenced_tasks,
            extra_env,
            task_run_state,
        })))
    }

    /// Also the cycle check: a cyclic graph cannot be scheduled, and
    /// sequenced into an arbitrary order it would succeed or fail by luck.
    fn sequence(&self, task_graph: &mut TaskGraph) -> miette::Result<Vec<TaskKey>> {
        Ok(sequence_tasks(
            task_graph,
            &SequenceTasksOptions {
                workspace_dir: self.workspace_root,
                ignore_cycles: self.config.ignore_workspace_cycles,
                emit: self.script.emit,
            },
        )?)
    }

    /// Decided on the graph as built, before the run-only resume and
    /// sequencing, which do not apply to `exec`.
    fn falls_back_to_exec(&self, task_graph: &TaskGraph) -> bool {
        self.script.fallback_to_exec
            && !self.args.if_present
            && !self.args.dry_run
            && !a_project_has_the_script(task_graph)
    }

    fn validate_requested_scripts(&self, task_graph: &mut TaskGraph) -> miette::Result<()> {
        // Hidden scripts (names starting with `.`) can only be invoked from
        // within another script, detected by an inherited
        // `npm_lifecycle_event`. Checked only for the tasks the invocation
        // named: a `dependsOn` declaration naming a hidden script is a
        // deliberate reference, like a call from another script.
        filter_hidden_requested_scripts(task_graph, self.script.script_name)?;

        check_a_project_has_the_script(
            task_graph,
            self.args,
            self.script.script_name,
            self.script.all_packages_selected,
        )?;

        Ok(())
    }

    fn task_graph(&self) -> miette::Result<TaskGraph> {
        let selector = ScriptSelector::new(self.script.script_name)?;
        build_run_task_graph(
            self.script.script_name,
            &selector,
            self.args,
            self.config,
            self.graph,
            self.selection,
            self.script.emit,
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
        let bail = !self.args.workspace.no_bail;
        let concurrency = run_concurrency(self.args, self.config, prepared.task_graph.len());
        let runs_concurrently = concurrency > 1
            && !is_serial_task_graph(&prepared.task_graph, &prepared.sequenced_tasks);
        let slots = RunSlots::queued(&prepared.task_graph);
        let process_tracker = run_process_tracker(bail, runs_concurrently);
        let script_budget = run_script_budget(self.args, self.config, &prepared.task_graph);
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
            bail,
            script_budget: &script_budget,
            inherit_output: !self.config.stream && !runs_concurrently,
            init_cwd: &init_cwd,
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
    full_task_graph
        .keys()
        .filter(|key| !task_graph.contains_key(*key))
        .cloned()
        .collect()
}

mod execution;

mod script_budget;

mod selection;
