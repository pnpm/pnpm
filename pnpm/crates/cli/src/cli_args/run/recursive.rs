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
        return Ok(());
    };
    // Report what the `--filter` selection resolved to before running a
    // single script, so the user can confirm it covers what they meant.
    emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: graph.len(),
        total: Some(projects.len()),
        workspace_prefix: config
            .workspace_dir
            .as_deref()
            .map(|dir| dir.to_string_lossy().into_owned()),
    }));
    // An empty `--filter` selection is a no-op (exit 0); an empty
    // workspace instead falls through to the no-script error below.
    if !projects.is_empty() && graph.is_empty() {
        return Ok(());
    }

    // Compiled once for the whole run, not per project or task.
    let selector = ScriptSelector::new(script_name)?;
    let full_task_graph =
        build_run_task_graph(script_name, &selector, args, config, graph, &selection, emit)?;
    let mut sync_injected = config.sync_injected_deps_after_scripts.clone();
    sync_injected.sort();
    let scripts_prepend_node_path = match config.scripts_prepend_node_path {
        pnpm_config::ScriptsPrependNodePath::Always => "true",
        pnpm_config::ScriptsPrependNodePath::Never => "false",
        pnpm_config::ScriptsPrependNodePath::WarnOnly => "warn-only",
    };
    let extra_env: HashMap<String, String> = config.extra_env_with_node_options();
    let mut state_settings = task_run_execution_settings(&TaskRunExecutionSettings {
        extra_bin_paths: &config.extra_bin_paths,
        extra_env: &extra_env,
        modules_dir: &config.modules_dir,
        node_experimental_package_map: config.node_experimental_package_map,
        node_options: config.node_options.as_deref(),
        user_agent: &config.user_agent,
    });
    state_settings.extend([
        format!("enable-pre-post-scripts={}", config.enable_pre_post_scripts),
        format!("script-shell={}", config.script_shell.as_deref().unwrap_or_default()),
        format!("scripts-prepend-node-path={scripts_prepend_node_path}"),
        format!("shell-emulator={}", config.shell_emulator),
        format!(
            "sync-injected-deps-after-scripts={}",
            serde_json::to_string(&sync_injected).expect("script names serialize"),
        ),
    ]);
    let task_run_state_context = TaskRunStateContext::new(
        "run",
        &args.script,
        &state_settings,
        &full_task_graph,
        workspace_root,
        |node, script| {
            let manifest = &graph[&node.project].package.project.manifest;
            let Some(main) =
                manifest.script(script, true).expect("if-present script lookup cannot fail")
            else {
                return Vec::new();
            };
            get_run_script_commands(manifest, script, main, config.enable_pre_post_scripts)
        },
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
    let mut task_graph = if let Some(anchor) = resume_anchor {
        resume_task_graph_from(
            full_task_graph.clone(),
            &anchor,
            script_name,
            completed_tasks.as_ref(),
        )
    } else {
        full_task_graph.clone()
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

    if args.dry_run {
        if args.json {
            let document = task_graph_to_json(&task_graph, workspace_root);
            println!("{}", serde_json::to_string_pretty(&document).into_diagnostic()?);
        } else {
            println!(
                "{}",
                render_task_graph_dry_run(&task_graph, &sequenced_tasks, workspace_root),
            );
        }
        return Ok(());
    }

    // Hidden scripts (names starting with `.`) can only be invoked from
    // within another script, detected by an inherited
    // `npm_lifecycle_event`. Checked only for the tasks the invocation
    // named: a `dependsOn` declaration naming a hidden script is a
    // deliberate reference, like a call from another script.
    if env::var_os("npm_lifecycle_event").is_none() {
        for node in task_graph.values_mut().filter(|node| node.requested) {
            node.scripts =
                throw_or_filter_hidden_scripts(std::mem::take(&mut node.scripts), script_name)?;
        }
    }

    // Before anything is dispatched: when no selected project has the
    // script, the run is a user error, and the tasks `dependsOn` pulled in
    // must not have run their side effects by the time it is reported.
    if script_name != "test"
        && !args.if_present
        && task_graph.values().all(|node| !node.requested || node.scripts.is_empty())
    {
        return Err(no_requested_script_error(script_name, graph.len() == projects.len()).into());
    }

    let initially_completed: HashSet<TaskKey> =
        full_task_graph.keys().filter(|key| !task_graph.contains_key(*key)).cloned().collect();
    let task_run_state = task_run_state_context.start(&initially_completed)?;

    let bail = !args.no_bail;
    let concurrency = if args.parallel {
        task_graph.len()
    } else if args.sequential {
        1
    } else {
        usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
    };
    let runs_concurrently = concurrency > 1 && !is_serial_task_graph(&task_graph, &sequenced_tasks);
    // pnpm pipes unless the output cannot interleave: `--stream` off, and
    // the graph cannot put two scripts in flight at once.
    let inherit_output = !config.stream && !runs_concurrently;

    let result: Mutex<IndexMap<String, ExecutionStatus>> = Mutex::new(
        task_graph
            .values()
            .map(|node| (task_summary_key(node), ExecutionStatus::queued()))
            .collect(),
    );
    let has_command = AtomicUsize::new(0);
    let first_failure: Mutex<Option<String>> = Mutex::new(None);
    let abort: Mutex<Option<miette::Report>> = Mutex::new(None);
    let process_tracker = bail.then(|| {
        if runs_concurrently { ProcessTracker::default() } else { ProcessTracker::foreground() }
    });

    let init_cwd = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());

    let run_task = |node: &TaskNode| -> TaskCompletion {
        let summary_key = task_summary_key(node);
        let on_started = || {
            result.lock().expect("summary lock is not poisoned")[&summary_key].status =
                Status::Running;
        };
        let execution = match run_project(RunProjectOptions {
            node,
            graph,
            args,
            init_cwd: &init_cwd,
            config,
            extra_env: &extra_env,
            bail,
            silent,
            inherit_output,
            emit,
            process_tracker: process_tracker.as_ref(),
            on_started: &on_started,
        }) {
            Ok(execution) => execution,
            Err(error) => {
                let mut abort = abort.lock().expect("abort slot lock is not poisoned");
                if abort.is_none() {
                    *abort = Some(error);
                }
                if let Some(process_tracker) = &process_tracker {
                    process_tracker.cancel();
                }
                return TaskCompletion::Aborted;
            }
        };
        if node.requested {
            has_command.fetch_add(execution.has_command, Ordering::Relaxed);
        }
        let failed = execution.status.status == Status::Failure;
        let cancelled = execution.cancelled;
        let recursion_guarded = execution.recursion_guarded;
        result.lock().expect("summary lock is not poisoned")[&summary_key] = execution.status;
        if cancelled {
            return TaskCompletion::Cancelled;
        }
        if failed {
            let mut first_failure =
                first_failure.lock().expect("first-failure slot lock is not poisoned");
            if first_failure.is_none() {
                *first_failure = Some(node.project.to_string_lossy().into_owned());
            }
            TaskCompletion::Failed
        } else if recursion_guarded {
            TaskCompletion::Passed
        } else if let Err(error) = task_run_state.record_passed(
            &TaskKey { project: node.project.clone(), task_name: node.task_name.clone() },
            node,
            workspace_root,
        ) {
            let mut abort = abort.lock().expect("abort slot lock is not poisoned");
            if abort.is_none() {
                *abort = Some(error);
            }
            if let Some(process_tracker) = &process_tracker {
                process_tracker.cancel();
            }
            TaskCompletion::Aborted
        } else {
            TaskCompletion::Passed
        }
    };
    let on_task_skipped = |node: &TaskNode| {
        result.lock().expect("summary lock is not poisoned")[&task_summary_key(node)].status =
            Status::Skipped;
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
    if bail
        && let Some(prefix) =
            first_failure.into_inner().expect("first-failure slot lock is not poisoned")
    {
        if args.report_summary {
            write_recursive_summary(workspace_root, &result)?;
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
    let failures = count_failures(&result);
    if script_name != "test"
        && has_command.load(Ordering::Relaxed) == 0
        && failures == 0
        && !args.if_present
    {
        task_run_state.finish()?;
        return Err(no_requested_script_error(script_name, graph.len() == projects.len()).into());
    }

    if args.report_summary {
        write_recursive_summary(workspace_root, &result)?;
    }

    if failures > 0 {
        return Err(RecursiveRunError::RecursiveFail { count: failures }.into());
    }
    task_run_state.finish()?;
    Ok(())
}

/// The task graph of this invocation: `script_name` in every selected
/// project plus what the `tasks` declarations pull in, with `--reverse`
/// applied.
///
/// `--no-sort` keeps its meaning of disregarding ordering entirely: tasks
/// get no edges, and the `tasks` declarations do not apply.
fn no_requested_script_error(script_name: &str, all_packages_selected: bool) -> RecursiveRunError {
    let script_name = script_name.to_string();
    if all_packages_selected {
        RecursiveRunError::NoScript { script_name }
    } else {
        RecursiveRunError::NoSelectedScript { script_name }
    }
}

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

fn run_project(options: RunProjectOptions<'_, '_>) -> miette::Result<ProjectExecution> {
    let RunProjectOptions {
        node,
        graph,
        args,
        init_cwd,
        config,
        extra_env,
        bail,
        silent,
        inherit_output,
        emit,
        process_tracker,
        on_started,
    } = options;
    let root = node.project.as_path();
    let manifest = &graph[root].package.project.manifest;
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
    for selected in &node.scripts {
        let Some(script) = manifest.script(selected, true)? else {
            continue;
        };
        if script.is_empty() || (args.script_args().is_empty() && script == "npx only-allow pnpm") {
            continue;
        }
        if env::var_os("npm_lifecycle_event").is_some_and(|event| event == **selected)
            && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
        {
            execution.recursion_guarded = true;
            continue;
        }

        on_started();
        if !project_failed {
            execution.status.status = Status::Running;
        }
        execution.has_command += 1;
        let start = Instant::now();
        let ctx = RunContext {
            manifest,
            dir: root,
            init_cwd,
            config,
            extra_env: &extra_env,
            silent,
            output: if inherit_output {
                ScriptOutput::Inherit
            } else {
                ScriptOutput::Streamed { dep_path: &root_str, emit }
            },
            process_tracker,
        };
        let status = run_stages(&ctx, selected, script, args.script_args())?;
        let duration = start.elapsed().as_secs_f64() * 1e3;

        if process_tracker.is_some_and(ProcessTracker::is_cancelled) {
            execution.cancelled = true;
            return Ok(execution);
        }

        if status.success() {
            if !project_failed {
                execution.status.status = Status::Passed;
                execution.status.duration = Some(duration);
            }
        } else {
            if process_tracker.is_some_and(|process_tracker| !process_tracker.cancel()) {
                execution.cancelled = true;
                return Ok(execution);
            }
            project_failed = true;
            execution.status.status = Status::Failure;
            execution.status.duration = Some(duration);
            execution.status.message =
                Some(format!("command failed with exit code {}", status.code().unwrap_or(1)));
            execution.status.prefix = Some(root.to_string_lossy().into_owned());
            if bail {
                break;
            }
        }
    }
    Ok(execution)
}
