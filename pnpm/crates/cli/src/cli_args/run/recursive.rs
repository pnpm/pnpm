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

/// The slots the tasks record into while they run.
struct RunSlots {
    result: Mutex<IndexMap<String, ExecutionStatus>>,
    has_command: AtomicUsize,
    first_failure: Mutex<Option<String>>,
    abort: Mutex<Option<miette::Report>>,
}

impl RunSlots {
    fn queued(task_graph: &TaskGraph) -> Self {
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

    fn into_results(self, bail: bool) -> miette::Result<RunResults> {
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
struct TaskRunner<'a, 'run, 'project> {
    run: &'a RecursiveRun<'run, 'project>,
    outcome: RunOutcome<'a>,
    extra_env: &'a HashMap<String, String>,
    init_cwd: &'a Path,
    bail: bool,
    /// pnpm pipes unless the output cannot interleave: `--stream` off, and
    /// the graph cannot put two scripts in flight at once.
    inherit_output: bool,
}

impl TaskRunner<'_, '_, '_> {
    fn run_task(&self, node: &TaskNode) -> TaskCompletion {
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

/// Before anything is dispatched: when no selected project has the
/// script, the run is a user error, and the tasks `dependsOn` pulled in
/// must not have run their side effects by the time it is reported.
///
/// `test` is exempt because `pnpm test` falls back to a default.
fn check_a_project_has_the_script(
    task_graph: &TaskGraph,
    args: &RunArgs,
    script_name: &str,
    all_packages_selected: bool,
) -> miette::Result<()> {
    if script_name == "test" || args.if_present {
        return Ok(());
    }
    if task_graph.values().any(|node| node.requested && !node.scripts.is_empty()) {
        return Ok(());
    }
    Err(no_requested_script_error(script_name, all_packages_selected).into())
}

/// `--no-bail` runs every task whatever fails, so it needs no tracker at
/// all. A serial graph runs its child in the foreground, where Ctrl-C
/// reaches it through the terminal instead.
fn run_process_tracker(bail: bool, runs_concurrently: bool) -> Option<ProcessTracker> {
    if !bail {
        return None;
    }
    Some(if runs_concurrently { ProcessTracker::default() } else { ProcessTracker::foreground() })
}

/// The settings that identify a run in the task-run state file: change
/// any of them and a previous run's state no longer describes this one.
fn run_state_settings(config: &Config, extra_env: &HashMap<String, String>) -> Vec<String> {
    let mut sync_injected = config.sync_injected_deps_after_scripts.clone();
    sync_injected.sort();
    let scripts_prepend_node_path = match config.scripts_prepend_node_path {
        pnpm_config::ScriptsPrependNodePath::Always => "true",
        pnpm_config::ScriptsPrependNodePath::Never => "false",
        pnpm_config::ScriptsPrependNodePath::WarnOnly => "warn-only",
    };
    let mut settings = task_run_execution_settings(&TaskRunExecutionSettings {
        extra_bin_paths: &config.extra_bin_paths,
        extra_env,
        modules_dir: &config.modules_dir,
        node_experimental_package_map: config.node_experimental_package_map,
        node_options: config.node_options.as_deref(),
        user_agent: &config.user_agent,
    });
    settings.extend([
        format!("enable-pre-post-scripts={}", config.enable_pre_post_scripts),
        format!("script-shell={}", config.script_shell.as_deref().unwrap_or_default()),
        format!("scripts-prepend-node-path={scripts_prepend_node_path}"),
        format!("shell-emulator={}", config.shell_emulator),
        format!(
            "sync-injected-deps-after-scripts={}",
            serde_json::to_string(&sync_injected).expect("script names serialize"),
        ),
    ]);
    settings
}

/// The task graph this run actually schedules: the full graph, or —
/// under `--resume-from` — the part of it that follows the named project,
/// minus what the previous run already completed.
fn resume_task_graph(
    task_run_state_context: &TaskRunStateContext,
    args: &RunArgs,
    graph: &ProjectGraph<GraphPkg<'_>>,
    full_task_graph: &TaskGraph,
    script_name: &str,
) -> miette::Result<TaskGraph> {
    let Some(resume_from) = args.resume_from.as_ref() else {
        return Ok(full_task_graph.clone());
    };
    let anchor = find_resume_root(resume_from, graph)?;
    let completed_tasks = task_run_state_context.read_completed_tasks()?;
    Ok(resume_task_graph_from(
        full_task_graph.clone(),
        &anchor,
        script_name,
        completed_tasks.as_ref(),
    ))
}

/// `pnpm -r run` with no script name lists the one selected project's
/// scripts; with several selected there is nothing to list.
fn print_selected_project_commands(
    graph: &ProjectGraph<GraphPkg<'_>>,
    projects: &[pnpm_workspace::Project],
    workspace_root: &Path,
) -> miette::Result<()> {
    if graph.len() != 1 {
        return Err(RecursiveRunError::ScriptNameRequired.into());
    }
    let project = graph.values().next().expect("graph contains exactly one project");
    let root_manifest = projects
        .iter()
        .find(|candidate| {
            candidate.root_dir == workspace_root
                && candidate.root_dir != project.package.project.root_dir
        })
        .map(|project| project.manifest.value());
    println!(
        "{}",
        render_project_commands(project.package.project.manifest.value(), root_manifest),
    );
    Ok(())
}

/// `--dry-run` prints the plan instead of running it.
fn print_run_dry_run(
    args: &RunArgs,
    task_graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    workspace_root: &Path,
) -> miette::Result<()> {
    if args.json {
        let document = task_graph_to_json(task_graph, workspace_root);
        println!("{}", serde_json::to_string_pretty(&document).into_diagnostic()?);
    } else {
        println!("{}", render_task_graph_dry_run(task_graph, sequenced_tasks, workspace_root));
    }
    Ok(())
}

/// Hidden scripts (names starting with `.`) can only be invoked from
/// within another script, detected by an inherited `npm_lifecycle_event`.
/// Checked only for the tasks the invocation named: a `dependsOn`
/// declaration naming a hidden script is a deliberate reference, like a
/// call from another script.
fn filter_hidden_requested_scripts(
    task_graph: &mut TaskGraph,
    script_name: &str,
) -> miette::Result<()> {
    if env::var_os("npm_lifecycle_event").is_some() {
        return Ok(());
    }
    for node in task_graph.values_mut().filter(|node| node.requested) {
        node.scripts =
            throw_or_filter_hidden_scripts(std::mem::take(&mut node.scripts), script_name)?;
    }
    Ok(())
}

/// How many tasks run at once. `--parallel` runs them all, `--sequential`
/// one at a time, and the default follows the workspace concurrency
/// setting.
fn run_concurrency(args: &RunArgs, config: &Config, task_count: usize) -> usize {
    if args.parallel {
        return task_count;
    }
    if args.sequential {
        return 1;
    }
    usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
}

/// Where each task's result is recorded.
struct RunOutcome<'a> {
    result: &'a Mutex<IndexMap<String, ExecutionStatus>>,
    has_command: &'a AtomicUsize,
    first_failure: &'a Mutex<Option<String>>,
    abort: &'a Mutex<Option<miette::Report>>,
    process_tracker: Option<&'a ProcessTracker>,
    task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
    workspace_root: &'a Path,
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
    fn abort(&self, error: miette::Report) -> TaskCompletion {
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

/// What the run reports once every task has settled.
struct RunReporting<'a> {
    args: &'a RunArgs,
    script_name: &'a str,
    workspace_root: &'a Path,
    all_packages_selected: bool,
    ran_a_command: bool,
    task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
}

fn report_run_outcome(
    reporting: &RunReporting<'_>,
    result: &IndexMap<String, ExecutionStatus>,
    bail_prefix: Option<String>,
) -> miette::Result<()> {
    let RunReporting { args, script_name, workspace_root, task_run_state, .. } = *reporting;
    if let Some(prefix) = bail_prefix {
        if args.report_summary {
            write_recursive_summary(workspace_root, result)?;
        }
        return Err(RecursiveRunError::RecursiveRunFirstFail { prefix }.into());
    }

    // `test` is exempt because `pnpm test` falls back to a default and
    // should not error on a workspace with no `test` script; otherwise a
    // recursive run that matched nothing is a user error, unless
    // `--if-present` opted out of it. The error is only for a run that had
    // nothing to do: a run where a `dependsOn`-pulled task failed and
    // skipped every requested task must report that failure instead of
    // claiming the script does not exist.
    let failures = count_failures(result);
    if script_name != "test" && !reporting.ran_a_command && failures == 0 && !args.if_present {
        task_run_state.finish()?;
        return Err(no_requested_script_error(script_name, reporting.all_packages_selected).into());
    }

    if args.report_summary {
        write_recursive_summary(workspace_root, result)?;
    }
    if failures > 0 {
        return Err(RecursiveRunError::RecursiveFail { count: failures }.into());
    }
    task_run_state.finish()
}

fn no_requested_script_error(script_name: &str, all_packages_selected: bool) -> RecursiveRunError {
    let script_name = script_name.to_string();
    if all_packages_selected {
        RecursiveRunError::NoScript { script_name }
    } else {
        RecursiveRunError::NoSelectedScript { script_name }
    }
}

/// The task graph of this invocation: `script_name` in every selected
/// project plus what the `tasks` declarations pull in, with `--reverse`
/// applied.
///
/// `--no-sort` keeps its meaning of disregarding ordering entirely: tasks
/// get no edges, and the `tasks` declarations do not apply.
fn build_run_task_graph(
    script_name: &str,
    selector: &ScriptSelector<'_>,
    args: &RunArgs,
    config: &Config,
    graph: &ProjectGraph<GraphPkg<'_>>,
    selection: &crate::cli_args::recursive::RecursiveSelection<'_>,
    emit: fn(&LogEvent),
) -> miette::Result<TaskGraph> {
    let project_dependencies: IndexMap<PathBuf, Vec<PathBuf>> = if args.sort {
        filtered_projects_dependencies(
            graph,
            selection.full_graph(),
            selection.prod_all.as_ref(),
            &selection.prod_only_selected,
        )
    } else {
        warn_ignored_task_declarations(config, emit);
        graph.keys().cloned().map(|root| (root, Vec::new())).collect()
    };
    let select_scripts = |project: &Path, task_name: &str| -> Vec<String> {
        let manifest = graph[project].package.project.manifest.value();
        if task_name == script_name {
            return selector.select(manifest);
        }
        // A task name `dependsOn` pulled in; a selector it cannot compile
        // reads as a plain name that matches nothing, like pnpm's.
        match ScriptSelector::new(task_name) {
            Ok(selector) => selector.select(manifest),
            Err(_) => Vec::new(),
        }
    };
    let mut task_graph = build_task_graph(&BuildTaskGraphOptions {
        project_dependencies: &project_dependencies,
        select_scripts,
        task_name: script_name,
        tasks: (args.sort && !config.tasks.is_empty()).then_some(&config.tasks),
    });
    if args.reverse {
        task_graph = reverse_task_graph(&task_graph);
    }
    Ok(task_graph)
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

fn warn_ignored_task_declarations(config: &Config, emit: fn(&LogEvent)) {
    if !config.tasks.is_empty() {
        emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: "The tasks declarations in pnpm-workspace.yaml are ignored because sorting is disabled (--no-sort or --parallel)".to_string(),
                prefix: config
                    .workspace_dir
                    .as_deref()
                    .unwrap_or_else(|| Path::new("."))
                    .to_string_lossy()
                    .into_owned(),
            }));
    }
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
