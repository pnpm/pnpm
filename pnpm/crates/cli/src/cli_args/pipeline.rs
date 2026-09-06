//! `pnpm pipeline` — run a named set of workspace tasks the way a CI run
//! would: affected-since-base selection as a pre-pass, the task graph in
//! dependency order without bailing, cached task results restored instead
//! of re-run, and a machine-readable account of what happened.
//!
//! Proof of concept for the `pnpm ci` RFC (pnpm/rfcs — "pnpm as the CI
//! engine"), with the task cache of pnpm/rfcs#22 in a local-tier-only
//! form. The command name is `pipeline` because `pnpm ci` is already the
//! clean-install command.

use super::{
    install::InstallArgs,
    recursive::{ExecutionStatus, Status, discover_workspace_projects},
    reporter::{ReporterType, reporter_emit},
    run::{RunContext, ScriptSelector, run_stages},
};
use crate::cli_args::recursive::filtered_projects_dependencies;
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_executor::ScriptOutput;
use pnpm_injected_deps_syncer::{SyncInjectedDeps, sync_injected_deps};
use pnpm_package_manager::{
    make_node_package_map_option, make_node_require_option, package_map_path_for_execution,
    pnp_path_for_execution,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_filter::{GetChangedProjectsOptions, get_changed_projects};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, ProjectGraph, create_projects_graph,
};
use pnpm_workspace_task_scheduler::{
    BuildPipelineTaskGraphOptions, ScheduleTasksOptions, SequenceTasksOptions, TaskCompletion,
    TaskGraph, TaskKey, TaskNode, build_pipeline_task_graph, format_task,
    render_task_graph_dry_run, schedule_tasks, sequence_tasks, task_graph_to_json,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::Instant,
};

mod agent;
mod cache;
mod capture;
mod cargo_cache;
mod paths;
mod report;

pub use agent::{WatchInvocation, run_watch};
use cache::{CacheDisposition, TaskCache};
use report::RunReport;
pub use report::RunUpload;

/// The base ref the affected selection falls back to when neither
/// `--base` nor the `pipelineBase` setting names one.
const DEFAULT_PIPELINE_BASE: &str = "origin/main";

/// The pipeline `pnpm pipeline` runs when no name is given.
const DEFAULT_PIPELINE_NAME: &str = "default";

#[derive(Debug, Args)]
pub struct PipelineArgs {
    /// The pipeline to run, from the `pipelines` section of
    /// `pnpm-workspace.yaml`. Defaults to "default".
    pub name: Option<String>,

    /// The install `pnpm pipeline` performs first is always a frozen
    /// install; these flags tune the rest of it. `--dry-run` prints the
    /// task graph without installing or running anything.
    #[clap(flatten)]
    pub install_args: InstallArgs,

    /// With `--dry-run`, print the tasks and their resolved dependency
    /// edges as JSON.
    #[clap(long)]
    pub json: bool,

    /// Run every task without reading or writing cached results or Cargo snapshots.
    #[clap(long = "no-cache")]
    pub no_cache: bool,

    /// Run the pipeline over every workspace project instead of the
    /// affected-since-base selection.
    #[clap(long)]
    pub full: bool,

    /// The git ref the affected selection diffs against (its merge base
    /// with HEAD). Overrides the `pipelineBase` setting.
    #[clap(long)]
    pub base: Option<String>,

    /// Publish the run's summary and event stream to the configured pnpr
    /// server (the `pnprServer` setting) once the run settles.
    #[clap(long)]
    pub report: bool,

    /// Publish the run to this pnpr server instead of the `pnprServer`
    /// setting — which also drives install offloading, so a server that
    /// only stores runs is better named here.
    #[clap(long = "report-to", value_name = "URL")]
    pub report_to: Option<String>,

    /// Watch a git repository and run the pipeline for every new revision
    /// of a branch, instead of running once against the current
    /// directory.
    #[clap(long, requires = "repo")]
    pub watch: bool,

    /// The repository the watch agent polls and builds — a URL or a local
    /// path, anything git accepts as a remote.
    #[clap(long, value_name = "REPO")]
    pub repo: Option<String>,

    /// The branch the watch agent follows.
    #[clap(long, default_value = "main", value_name = "NAME")]
    pub branch: String,

    /// Seconds between polls of the watched repository.
    #[clap(long, default_value_t = 30, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub interval: u64,

    /// With `--watch`: poll once, build if there is a new revision, and
    /// exit.
    #[clap(long, requires = "watch")]
    pub once: bool,
}

/// The pipeline-specific inputs of one invocation, split off
/// [`PipelineArgs`] once the install half has been consumed.
pub struct PipelineInvocation {
    pub name: Option<String>,
    pub dry_run: bool,
    pub json: bool,
    pub no_cache: bool,
    pub full: bool,
    pub base: Option<String>,
    pub report: bool,
    pub report_to: Option<String>,
}

/// How the run ended, and what a `--report` submission would carry. The
/// failure exit is raised by the dispatcher after any reporting, so a
/// failed run is still recorded.
pub struct PipelineOutcome {
    pub failed_tasks: usize,
    pub upload: Option<RunUpload>,
}

impl PipelineOutcome {
    fn without_upload() -> Self {
        PipelineOutcome { failed_tasks: 0, upload: None }
    }
}

/// The identity runs are recorded under on the server: the workspace
/// directory's name plus the same path hash that keys the local pipeline
/// data, so two checkouts of one repository stay distinguishable.
fn workspace_identity(workspace_root: &Path) -> String {
    let basename: String = workspace_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(50)
        .collect();
    let slug = pnpm_crypto_hash::create_short_hash(&workspace_root.to_string_lossy());
    let basename = basename.trim_start_matches('.');
    if basename.is_empty() { slug } else { format!("{basename}-{slug}") }
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PipelineError {
    #[display("No pipelines are defined in pnpm-workspace.yaml")]
    #[diagnostic(
        code(ERR_PNPM_NO_PIPELINES),
        help(
            "Declare one under the \"pipelines\" key, e.g.\n\npipelines:\n  check:\n    - lint\n    - build\n    - test"
        )
    )]
    NoPipelines,

    #[display("There is no pipeline named \"{name}\". Available pipelines: {available}")]
    #[diagnostic(code(ERR_PNPM_UNKNOWN_PIPELINE))]
    UnknownPipeline { name: String, available: String },

    #[display("\"pnpm pipeline\" failed in {count} tasks")]
    #[diagnostic(code(ERR_PNPM_PIPELINE_FAIL))]
    PipelineFail {
        #[error(not(source))]
        count: usize,
    },
}

/// Run the pipeline. The frozen install has already happened by the time
/// this is called (see `dispatch_install::pipeline`); this is selection,
/// graph, cache, and report.
pub fn run_pipeline(
    invocation: &PipelineInvocation,
    config: &Config,
    dir: &Path,
    reporter: ReporterType,
) -> miette::Result<PipelineOutcome> {
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
    let emit = reporter_emit(reporter);
    let silent = matches!(reporter, ReporterType::Ndjson | ReporterType::Silent);
    let info = |message: String| {
        emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message,
            prefix: workspace_root.to_string_lossy().into_owned(),
        }));
    };

    if config.pipelines.is_empty() {
        return Err(PipelineError::NoPipelines.into());
    }
    let name = invocation.name.as_deref().unwrap_or(DEFAULT_PIPELINE_NAME);
    let Some(requested_tasks) = config.pipelines.get(name) else {
        return Err(PipelineError::UnknownPipeline {
            name: name.to_string(),
            available: config.pipelines.keys().cloned().collect::<Vec<_>>().join(", "),
        }
        .into());
    };

    let (projects, _patterns) = discover_workspace_projects(workspace_root, config)?;
    let graph = build_full_graph(&projects, config);

    let base = invocation
        .base
        .clone()
        .or_else(|| config.pipeline_base.clone())
        .unwrap_or_else(|| DEFAULT_PIPELINE_BASE.to_string());
    let selection = select_affected_projects(&SelectAffectedOptions {
        graph: &graph,
        workspace_root,
        base: &base,
        full: invocation.full,
        config,
        emit,
    })?;

    let revision = git_stdout(workspace_root, &["rev-parse", "HEAD"]);
    let report = RunReport::new(name, &base, &selection, revision)?;

    if selection.requested.is_empty() {
        info(format!("No projects are affected since {base} — nothing to run."));
        let report_dir = report.write(&pipeline_data_dir(config, workspace_root))?;
        info(format!("Report: {}", report_dir.display()));
        return Ok(PipelineOutcome {
            failed_tasks: 0,
            upload: Some(report.to_upload(workspace_identity(workspace_root))),
        });
    }

    let selected_graph: ProjectGraph<GraphPkg<'_>> = graph
        .iter()
        .filter(|(root, _)| selection.selected.contains(root.as_path()))
        .map(|(root, node)| (root.clone(), node.clone()))
        .collect();
    let project_dependencies =
        filtered_projects_dependencies(&selected_graph, &graph, None, &HashSet::new());

    let select_scripts = |project: &Path, task_name: &str| -> Vec<String> {
        let manifest = graph[project].package.project.manifest.value();
        match ScriptSelector::new(task_name) {
            Ok(selector) => selector.select(manifest),
            Err(_) => Vec::new(),
        }
    };
    let task_names: Vec<&str> = requested_tasks.iter().map(String::as_str).collect();
    let mut task_graph = build_pipeline_task_graph(&BuildPipelineTaskGraphOptions {
        project_dependencies: &project_dependencies,
        select_scripts,
        task_names: &task_names,
        requested_projects: Some(&selection.requested),
        tasks: (!config.tasks.is_empty()).then_some(&config.tasks),
    });
    let sequenced_tasks = sequence_tasks(
        &mut task_graph,
        &SequenceTasksOptions {
            workspace_dir: workspace_root,
            ignore_cycles: config.ignore_workspace_cycles,
            emit,
        },
    )?;

    if invocation.dry_run {
        if invocation.json {
            let document = task_graph_to_json(&task_graph, workspace_root);
            println!("{}", serde_json::to_string_pretty(&document).into_diagnostic()?);
        } else {
            println!(
                "{}",
                render_task_graph_dry_run(&task_graph, &sequenced_tasks, workspace_root),
            );
        }
        return Ok(PipelineOutcome::without_upload());
    }

    let cache = TaskCache::open(&pipeline_data_dir(config, workspace_root), workspace_root)?;
    // Keys are computed for every task before anything runs, walking the
    // sequenced order so a task's dependency keys exist when its own is
    // built. This is also what a distributed tier would need: the whole
    // plan, priced, without executing.
    let task_keys = compute_task_keys(&task_graph, &sequenced_tasks, &graph, &cache, config)?;

    capture::install_forward(emit);
    let concurrency = usize::try_from(config.workspace_concurrency).unwrap_or(usize::MAX).max(1);
    let init_cwd = env::current_dir().unwrap_or_else(|_| dir.to_path_buf());
    let base_extra_env: HashMap<String, String> = config.extra_env_with_node_options();

    let statuses: Mutex<IndexMap<String, ExecutionStatus>> = Mutex::new(
        task_graph
            .keys()
            .map(|key| (format_task(key, workspace_root), ExecutionStatus::queued()))
            .collect(),
    );
    let abort: Mutex<Option<miette::Report>> = Mutex::new(None);

    let run_task = |node: &TaskNode| -> TaskCompletion {
        let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
        let summary_key = format_task(&key, workspace_root);
        let outcome = run_pipeline_task(&RunTaskOptions {
            node,
            graph: &graph,
            config,
            invocation,
            cache: &cache,
            task_key: &task_keys[&key],
            init_cwd: &init_cwd,
            base_extra_env: &base_extra_env,
            emit,
            silent,
            report: &report,
            summary_key: &summary_key,
        });
        match outcome {
            Ok(status) => {
                let failed = status.status == Status::Failure;
                statuses.lock().expect("status lock is not poisoned")[&summary_key] = status;
                if failed { TaskCompletion::Failed } else { TaskCompletion::Passed }
            }
            Err(error) => {
                let mut abort = abort.lock().expect("abort slot lock is not poisoned");
                if abort.is_none() {
                    *abort = Some(error);
                }
                TaskCompletion::Aborted
            }
        }
    };
    let on_task_skipped = |node: &TaskNode| {
        let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
        let summary_key = format_task(&key, workspace_root);
        statuses.lock().expect("status lock is not poisoned")[&summary_key].status =
            Status::Skipped;
        report.task_skipped(&summary_key);
    };
    schedule_tasks(
        &task_graph,
        &ScheduleTasksOptions {
            concurrency,
            bail: false,
            run_task: &run_task,
            on_task_skipped: &on_task_skipped,
        },
    );

    if let Some(error) = abort.into_inner().expect("abort slot lock is not poisoned") {
        return Err(error);
    }

    let statuses = statuses.into_inner().expect("status lock is not poisoned");
    let failed = statuses.values().filter(|status| status.status == Status::Failure).count();
    let passed = statuses.values().filter(|status| status.status == Status::Passed).count();
    let skipped = statuses.values().filter(|status| status.status == Status::Skipped).count();
    let hits = report.cache_hits();

    report.finish(&statuses, &task_keys, workspace_root);
    let report_dir = report.write(&pipeline_data_dir(config, workspace_root))?;

    info(format!(
        r#"Pipeline "{name}": {total} tasks — {passed} passed ({hits} from cache), {failed} failed, {skipped} skipped."#,
        total = statuses.len(),
    ));
    info(format!("Report: {}", report_dir.display()));

    Ok(PipelineOutcome {
        failed_tasks: failed,
        upload: Some(report.to_upload(workspace_identity(workspace_root))),
    })
}

/// Where the pipeline keeps its task cache, restore records, and run
/// reports: under pnpm's cache directory, keyed by workspace path, so an
/// install pruning `node_modules` cannot take the cache with it.
fn pipeline_data_dir(config: &Config, workspace_root: &Path) -> PathBuf {
    let workspace_slug = pnpm_crypto_hash::create_short_hash(&workspace_root.to_string_lossy());
    config.cache_dir.join("pipeline").join(workspace_slug)
}

fn build_full_graph<'a>(projects: &'a [Project], config: &Config) -> ProjectGraph<GraphPkg<'a>> {
    let graph_options = CreateProjectsGraphOptions {
        link_workspace_packages: Some(
            config.link_workspace_packages != pnpm_config::LinkWorkspacePackages::Off,
        ),
        ..CreateProjectsGraphOptions::default()
    };
    create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &graph_options,
    )
    .graph
}

/// How the run decided what to cover.
pub struct Selection {
    /// The projects whose pipeline tasks the run requests: the changed
    /// projects and their dependents. Root tasks participate when
    /// `includeWorkspaceRoot` is enabled.
    pub requested: Vec<PathBuf>,
    /// The dependency closure of `requested` — the projects the task
    /// graph spans. The extra projects participate only through
    /// `dependsOn` edges (an upstream build pulled in without its lint or
    /// tests), which is what keeps a task's cache key independent of how
    /// the run was narrowed, and what guarantees a selected build's
    /// upstream outputs exist on a fresh machine.
    pub selected: HashSet<PathBuf>,
    pub mode: SelectionMode,
    pub merge_base: Option<String>,
    pub changed_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Changed projects and their dependents.
    Affected,
    /// Every project: `--full`, an unresolvable base, or a change to
    /// files no project selection can attribute (the workspace root).
    Full,
}

struct SelectAffectedOptions<'a> {
    graph: &'a ProjectGraph<GraphPkg<'a>>,
    workspace_root: &'a Path,
    base: &'a str,
    full: bool,
    config: &'a Config,
    emit: fn(&LogEvent),
}

/// The selection pre-pass: changed projects since the merge base, plus
/// their transitive dependents. It is an optimization, not the
/// correctness boundary — any doubt about attribution (the merge base
/// cannot be resolved, or the diff touches the workspace root, whose
/// files feed every project) falls through to the full graph.
fn select_affected_projects(options: &SelectAffectedOptions<'_>) -> miette::Result<Selection> {
    let SelectAffectedOptions { graph, workspace_root, base, full, config, emit } = *options;
    let all_dirs: Vec<PathBuf> = graph
        .keys()
        .filter(|dir| config.include_workspace_root || dir.as_path() != workspace_root)
        .cloned()
        .collect();
    let full_selection = |merge_base: Option<String>, changed_count: usize| Selection {
        requested: all_dirs.clone(),
        selected: all_dirs.iter().cloned().collect(),
        mode: SelectionMode::Full,
        merge_base,
        changed_count,
    };

    if full {
        return Ok(full_selection(None, 0));
    }
    let Some(merge_base) = resolve_merge_base(workspace_root, base) else {
        emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                "Cannot resolve the merge base of HEAD and {base}; running the pipeline over every project.",
            ),
            prefix: workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(None, 0));
    };

    let changed = get_changed_projects(
        graph.keys().cloned().collect(),
        &merge_base,
        &GetChangedProjectsOptions {
            workspace_dir: workspace_root,
            test_pattern: &config.test_pattern,
            changed_files_ignore_pattern: &config.changed_files_ignore_pattern,
        },
    )
    .map_err(miette::Report::new)?;
    let changed_count =
        changed.changed_projects.len() + changed.ignore_dependent_for_projects.len();

    // A changed file above every package maps to the workspace root
    // project: the root manifest, the lockfile, a shared config. Those
    // feed every project in ways project topology cannot see, so pruning
    // is disabled for the run rather than guessed at.
    if changed
        .changed_projects
        .iter()
        .chain(&changed.ignore_dependent_for_projects)
        .any(|dir| dir == workspace_root)
    {
        emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message:
                "The diff touches workspace-root files; running the pipeline over every project."
                    .to_string(),
            prefix: workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(Some(merge_base), changed_count));
    }

    let mut dependents: HashMap<&Path, Vec<&Path>> = HashMap::new();
    for (dir, node) in graph {
        for dependency in &node.dependencies {
            dependents.entry(dependency.as_path()).or_default().push(dir.as_path());
        }
    }
    let mut affected: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<&Path> = changed.changed_projects.iter().map(PathBuf::as_path).collect();
    while let Some(dir) = stack.pop() {
        if !affected.insert(dir.to_path_buf()) {
            continue;
        }
        stack.extend(dependents.get(dir).into_iter().flatten());
    }
    // A project whose only changes match `testPattern` is selected itself
    // without pulling in its dependents.
    affected.extend(changed.ignore_dependent_for_projects.iter().cloned());
    if !config.include_workspace_root {
        affected.remove(workspace_root);
    }

    // The task graph additionally spans the affected set's transitive
    // dependencies; see [`Selection::selected`] for why.
    let mut selected = affected.clone();
    let mut stack: Vec<PathBuf> = affected.iter().cloned().collect();
    while let Some(dir) = stack.pop() {
        for dependency in
            graph.get(&dir).map(|node| node.dependencies.as_slice()).unwrap_or_default()
        {
            if (config.include_workspace_root || dependency.as_path() != workspace_root)
                && selected.insert(dependency.clone())
            {
                stack.push(dependency.clone());
            }
        }
    }

    // In the workspace graph's deterministic order, which is the
    // dispatch tie-break order.
    let requested: Vec<PathBuf> =
        graph.keys().filter(|dir| affected.contains(dir.as_path())).cloned().collect();
    Ok(Selection {
        requested,
        selected,
        mode: SelectionMode::Affected,
        merge_base: Some(merge_base),
        changed_count,
    })
}

/// `merge-base(HEAD, base)`, deepening a shallow clone until the merge
/// base is reachable. `None` when it cannot be resolved (an unknown ref,
/// unrelated histories, not a git repository) — the caller falls back to
/// the full graph.
fn resolve_merge_base(workspace_root: &Path, base: &str) -> Option<String> {
    for _ in 0..5 {
        if let Some(sha) = git_stdout(workspace_root, &["merge-base", "HEAD", base]) {
            return Some(sha);
        }
        let shallow = git_stdout(workspace_root, &["rev-parse", "--is-shallow-repository"]);
        if shallow.as_deref() != Some("true") {
            return None;
        }
        let _ = Command::new("git")
            .args(["fetch", "--deepen=200"])
            .current_dir(workspace_root)
            .status();
    }
    None
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// Pass-through tasks contribute keys to invalidate their dependents.
fn compute_task_keys(
    task_graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    graph: &ProjectGraph<GraphPkg<'_>>,
    cache: &TaskCache,
    config: &Config,
) -> miette::Result<HashMap<TaskKey, String>> {
    let mut keys: HashMap<TaskKey, String> = HashMap::with_capacity(task_graph.len());
    for key in sequenced_tasks {
        let node = &task_graph[key];
        let manifest = graph[node.project.as_path()].package.project.manifest.value();
        let script_bodies = task_script_bodies(node, manifest, config.enable_pre_post_scripts);
        let mut dependency_keys: Vec<&str> =
            node.dependencies.iter().map(|dependency| keys[dependency].as_str()).collect();
        dependency_keys.sort_unstable();
        let task_key = cache.compute_task_key(&cache::TaskKeyInputs {
            node,
            settings: config.tasks.get(&node.task_name),
            dependency_keys: &dependency_keys,
            script_bodies: &script_bodies,
            environment: &task_environment(
                config,
                &node.project,
                &config.extra_env_with_node_options(),
            ),
        })?;
        keys.insert(key.clone(), task_key);
    }
    Ok(keys)
}

fn task_script_bodies(
    node: &TaskNode,
    manifest: &Value,
    enable_pre_post_scripts: bool,
) -> Vec<(String, String)> {
    let mut bodies: Vec<(String, String)> = Vec::new();
    for script in &node.scripts {
        let stages: Vec<String> = if enable_pre_post_scripts {
            vec![format!("pre{script}"), script.clone(), format!("post{script}")]
        } else {
            vec![script.clone()]
        };
        for stage in stages {
            if let Some(body) = manifest
                .get("scripts")
                .and_then(|scripts| scripts.get(&stage))
                .and_then(Value::as_str)
            {
                bodies.push((stage, body.to_string()));
            }
        }
    }
    bodies
}

#[derive(Clone, Copy)]
struct RunTaskOptions<'a, 'graph> {
    node: &'a TaskNode,
    graph: &'a ProjectGraph<GraphPkg<'graph>>,
    config: &'a Config,
    invocation: &'a PipelineInvocation,
    cache: &'a TaskCache,
    task_key: &'a str,
    init_cwd: &'a Path,
    base_extra_env: &'a HashMap<String, String>,
    emit: fn(&LogEvent),
    silent: bool,
    report: &'a RunReport,
    summary_key: &'a str,
}

/// Script failures are returned as statuses. Infrastructure errors abort the run.
fn run_pipeline_task(options: &RunTaskOptions<'_, '_>) -> miette::Result<ExecutionStatus> {
    let RunTaskOptions {
        node,
        graph,
        config,
        invocation,
        cache,
        task_key,
        emit,
        report,
        summary_key,
        ..
    } = *options;
    let root = node.project.as_path();
    let settings = config.tasks.get(&node.task_name);
    let cacheable = task_cacheable(invocation, settings);
    let start = Instant::now();
    report.task_started(summary_key, task_key);

    if cacheable && let Some(stored) = cache.lookup(task_key) {
        match cache.restore(&stored, root, summary_key) {
            Ok(()) => {
                capture::replay(&stored.scripts, root, emit);
                sync_injected_deps_if_configured(config, node, graph)?;
                let duration = start.elapsed().as_secs_f64() * 1e3;
                report.task_finished(summary_key, Status::Passed, CacheDisposition::Hit, duration);
                emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Info,
                    message: format!("{summary_key}: restored from cache"),
                    prefix: root.to_string_lossy().into_owned(),
                }));
                return Ok(ExecutionStatus {
                    status: Status::Passed,
                    duration: Some(duration),
                    prefix: None,
                    message: None,
                });
            }
            Err(reason) => {
                // A file the restore cannot account for is the user's;
                // overwriting it silently is how caches lose trust. The
                // task runs normally instead.
                emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Warn,
                    message: format!("{summary_key}: not restoring from cache: {reason}"),
                    prefix: root.to_string_lossy().into_owned(),
                }));
            }
        }
    }

    let execution = execute_task_with_cargo_cache(options, settings)?;
    let duration = start.elapsed().as_secs_f64() * 1e3;
    if execution.status == Status::Passed
        && cacheable
        && let Some(captured) = execution.captured
    {
        let outputs = settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default();
        if let Err(error) = cache.store(task_key, root, summary_key, outputs, captured) {
            emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("{summary_key}: failed to store the task in the cache: {error}"),
                prefix: root.to_string_lossy().into_owned(),
            }));
        }
    }
    let disposition = if cacheable { CacheDisposition::Miss } else { CacheDisposition::Bypass };
    report.task_finished(summary_key, execution.status, disposition, duration);
    Ok(ExecutionStatus {
        status: execution.status,
        duration: Some(duration),
        prefix: (execution.status == Status::Failure).then(|| root.to_string_lossy().into_owned()),
        message: execution.message,
    })
}

fn execute_task_with_cargo_cache(
    options: &RunTaskOptions<'_, '_>,
    settings: Option<&pnpm_config::TaskSettings>,
) -> miette::Result<TaskExecution> {
    let RunTaskOptions { node, config, invocation, task_key, emit, summary_key, .. } = *options;
    let root = node.project.as_path();
    let Some(directory) = settings.and_then(|settings| settings.cargo_target_dir.as_deref()) else {
        return execute_task_scripts(options);
    };
    let cargo = cargo_cache::CargoCache::open(root, directory).into_diagnostic()?;
    let environment = cargo_cache::cache_environment(
        options.base_extra_env,
        settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default(),
    );
    let cargo_cacheable =
        !invocation.no_cache && settings.is_some_and(|settings| settings.cache != Some(false));
    let snapshot = if cargo_cacheable {
        match cargo_cache::snapshot_entry(&config.cache_dir, root, task_key, &environment) {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                cargo_cache_warning(options, &error.to_string());
                None
            }
        }
    } else {
        None
    };
    if let Some((entry, key, _)) = &snapshot {
        match cargo.restore(entry, key) {
            Ok(true) => emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: format!("{summary_key}: restored Cargo build state"),
                prefix: root.to_string_lossy().into_owned(),
            })),
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => cargo_cache_warning(options, &error.to_string()),
        }
        cargo.prepare(key).into_diagnostic()?;
    }
    let mut extra_env = options.base_extra_env.clone();
    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"] {
        extra_env.insert(name.to_string(), cargo.target.to_string_lossy().into_owned());
    }
    let execution =
        execute_task_scripts(&RunTaskOptions { base_extra_env: &extra_env, ..*options })?;
    if execution.status == Status::Passed
        && let Some((entry, key, local_packages)) = &snapshot
    {
        match cargo_cache::snapshot_entry(&config.cache_dir, root, task_key, &environment) {
            Ok((_, after, _)) if after == *key => {
                if let Err(error) = cargo.publish(entry, key, local_packages) {
                    cargo_cache_warning(options, &error.to_string());
                }
            }
            Ok(_) => cargo_cache_warning(
                options,
                "inputs changed during execution; snapshot was not saved",
            ),
            Err(error) => cargo_cache_warning(options, &error.to_string()),
        }
    }
    Ok(execution)
}

fn cargo_cache_warning(options: &RunTaskOptions<'_, '_>, reason: &str) {
    (options.emit)(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!("{}: Cargo build cache: {reason}", options.summary_key),
        prefix: options.node.project.to_string_lossy().into_owned(),
    }));
}

struct TaskExecution {
    status: Status,
    message: Option<String>,
    captured: Option<Vec<capture::CapturedScript>>,
}

/// Run the task's scripts for real, capturing their output stream for
/// the cache alongside the live reporter rendering.
fn execute_task_scripts(options: &RunTaskOptions<'_, '_>) -> miette::Result<TaskExecution> {
    let RunTaskOptions {
        node,
        graph,
        config,
        invocation,
        init_cwd,
        base_extra_env,
        silent,
        emit,
        ..
    } = *options;
    let root = node.project.as_path();
    let manifest = &graph[root].package.project.manifest;

    let extra_env = task_environment(config, root, base_extra_env);
    let capture_output = task_cacheable(invocation, config.tasks.get(&node.task_name));
    let root_str = root.to_string_lossy().into_owned();
    let mut status = Status::Passed;
    let mut message = None;
    let mut captured = capture_output.then(Vec::new);
    let mut captured_bytes = 0usize;
    for selected in &node.scripts {
        let Some(script) = manifest.script(selected, true).map_err(miette::Report::new)? else {
            continue;
        };
        if script.is_empty() || script == "npx only-allow pnpm" {
            continue;
        }
        if env::var_os("npm_lifecycle_event").is_some_and(|event| event == **selected)
            && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
        {
            continue;
        }
        let ctx = RunContext {
            manifest,
            dir: root,
            init_cwd,
            config,
            extra_env: &extra_env,
            silent,
            output: ScriptOutput::Streamed {
                dep_path: &root_str,
                emit: if capture_output { capture::capturing_emit } else { emit },
            },
            // The pipeline never bails, so there is no cancellation to
            // propagate into running children.
            process_tracker: None,
        };
        let exit = run_stages(&ctx, selected, script, &[]);
        if capture_output {
            match capture::drain_task(&root_str, selected, config.enable_pre_post_scripts) {
                Some(stages) => {
                    captured_bytes += stages
                        .iter()
                        .flat_map(|stage| &stage.lines)
                        .map(|line| line.line.len() + std::mem::size_of::<capture::CapturedLine>())
                        .sum::<usize>();
                    if captured_bytes > capture::MAX_CAPTURE_BYTES {
                        captured = None;
                    }
                    if let Some(captured) = &mut captured {
                        captured.extend(stages);
                    }
                }
                None => captured = None,
            }
        }
        let exit = exit?;
        if !exit.success() {
            status = Status::Failure;
            message = Some(format!("command failed with exit code {}", exit.code().unwrap_or(1)));
            break;
        }
    }
    Ok(TaskExecution { status, message, captured })
}

/// A cache hit skips the script but must not skip the injected-deps sync
/// that would have followed it — consumers of an injected dependency
/// would otherwise keep seeing the previous build.
fn sync_injected_deps_if_configured(
    config: &Config,
    node: &TaskNode,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> miette::Result<()> {
    if !config.sync_injected_deps_after_scripts.iter().any(|script| node.scripts.contains(script)) {
        return Ok(());
    }
    let manifest = graph[node.project.as_path()].package.project.manifest.value();
    sync_injected_deps(&SyncInjectedDeps {
        pkg_name: manifest.get("name").and_then(Value::as_str),
        pkg_root_dir: &node.project,
        workspace_dir: config.workspace_dir.as_deref(),
        manifest_before_scripts: Some(manifest),
    })?;
    Ok(())
}

fn task_cacheable(
    invocation: &PipelineInvocation,
    settings: Option<&pnpm_config::TaskSettings>,
) -> bool {
    !invocation.no_cache
        && settings.is_some_and(|settings| {
            settings.outputs.is_some()
                && settings.cache != Some(false)
                && settings.cargo_target_dir.is_none()
        })
}

fn task_environment(
    config: &Config,
    root: &Path,
    base_extra_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
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
