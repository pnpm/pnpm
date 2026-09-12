use super::{
    BuildTaskGraphOptions, Config, ExecutionStatus, GraphPkg, HashMap, IndexMap, IntoDiagnostic,
    LogEvent, LogLevel, Path, PathBuf, PnpmLog, ProcessTracker, ProjectGraph, RecursiveRunError,
    RunArgs, ScriptSelector, TaskGraph, TaskKey, TaskRunExecutionSettings, TaskRunStateContext,
    build_task_graph, count_failures, env, filtered_projects_dependencies, find_resume_root,
    render_project_commands, render_task_graph_dry_run, resume_task_graph_from, reverse_task_graph,
    task_graph_to_json, task_run_execution_settings, throw_or_filter_hidden_scripts,
    write_recursive_summary,
};

/// Before anything is dispatched: when no selected project has the
/// script, the run is a user error, and the tasks `dependsOn` pulled in
/// must not have run their side effects by the time it is reported.
///
/// `test` is exempt because `pnpm test` falls back to a default.
pub(super) fn check_a_project_has_the_script(
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
pub(super) fn run_process_tracker(bail: bool, runs_concurrently: bool) -> Option<ProcessTracker> {
    if !bail {
        return None;
    }
    Some(if runs_concurrently { ProcessTracker::default() } else { ProcessTracker::foreground() })
}

/// The settings that identify a run in the task-run state file: change
/// any of them and a previous run's state no longer describes this one.
pub(super) fn run_state_settings(
    config: &Config,
    extra_env: &HashMap<String, String>,
) -> Vec<String> {
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
pub(super) fn resume_task_graph(
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
pub(super) fn print_selected_project_commands(
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
pub(super) fn print_run_dry_run(
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
pub(super) fn filter_hidden_requested_scripts(
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
pub(super) fn run_concurrency(args: &RunArgs, config: &Config, task_count: usize) -> usize {
    if args.parallel {
        return task_count;
    }
    if args.sequential {
        return 1;
    }
    usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1)
}

/// What the run reports once every task has settled.
pub(super) struct RunReporting<'a> {
    pub(super) args: &'a RunArgs,
    pub(super) script_name: &'a str,
    pub(super) workspace_root: &'a Path,
    pub(super) all_packages_selected: bool,
    pub(super) ran_a_command: bool,
    pub(super) task_run_state: &'a crate::cli_args::task_run_state::TaskRunState,
}

pub(super) fn report_run_outcome(
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
pub(super) fn build_run_task_graph(
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
        requested_projects: None,
        tasks: (args.sort && !config.tasks.is_empty()).then_some(&config.tasks),
    });
    if args.reverse {
        task_graph = reverse_task_graph(&task_graph);
    }
    Ok(task_graph)
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
