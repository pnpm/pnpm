//! `pnpm pipeline` — run a named set of workspace tasks the way a CI run
//! would: affected-since-base selection as a pre-pass, the task graph in
//! dependency order without bailing, cached task results restored instead
//! of re-run, and a machine-readable account of what happened.
//!
//! Proof of concept for the `pnpm ci` RFC (pnpm/rfcs — "pnpm as the CI
//! engine"), with the task cache of pnpm/rfcs#22 in a local-tier-only
//! form. The command name is `pipeline` because `pnpm ci` is already the
//! clean-install command.

pub use agent::{WatchInvocation, run_watch};
pub use report::RunUpload;

use super::{
    catalogs::configured_catalogs,
    install::InstallArgs,
    recursive::{ExecutionStatus, Status, discover_workspace_projects},
    reporter::{ReporterType, reporter_emit},
    run::{RunContext, ScriptSelector, run_stages},
};
use crate::cli_args::recursive::filtered_projects_dependencies;
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_executor::ScriptOutput;
use pnpm_injected_deps_syncer::{SyncInjectedDeps, sync_injected_deps};
use pnpm_package_manager::{
    make_node_package_map_option, make_node_require_option, overrides_dependency_rewriter,
    package_map_path_for_execution, pnp_path_for_execution,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_filter::{GetChangedProjectsOptions, get_changed_projects};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, DependencyRewriter, ProjectGraph, create_projects_graph,
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
use cache::{CacheDisposition, TaskCache};
use report::RunReport;

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
    let run = PipelineRun {
        invocation,
        config,
        dir,
        workspace_root: config.workspace_dir.as_deref().unwrap_or(dir),
        emit: reporter_emit(reporter),
        silent: matches!(reporter, ReporterType::Ndjson | ReporterType::Silent),
    };
    let (name, requested_tasks) = run.requested_tasks()?;

    let (projects, _) = discover_workspace_projects(run.workspace_root, config)?;
    let graph = build_full_graph(&projects, config, run.workspace_root)?;

    let base = invocation
        .base
        .clone()
        .or_else(|| config.pipeline_base.clone())
        .unwrap_or_else(|| DEFAULT_PIPELINE_BASE.to_string());
    let selection = select_affected_projects(&SelectAffectedOptions {
        graph: &graph,
        workspace_root: run.workspace_root,
        base: &base,
        full: invocation.full,
        config,
        emit: run.emit,
    })?;

    let report = RunReport::new(
        name,
        &base,
        &selection,
        git_stdout(run.workspace_root, &["rev-parse", "HEAD"]),
    )?;

    if selection.requested.is_empty() {
        run.info(format!("No projects are affected since {base} — nothing to run."));
        return run.conclude(&report, None, 0);
    }
    run.execute(&PipelinePlan {
        name,
        requested_tasks,
        graph: &graph,
        selection: &selection,
        report: &report,
    })
}

/// What every phase of one pipeline run reads.
struct PipelineRun<'a> {
    invocation: &'a PipelineInvocation,
    config: &'a Config,
    dir: &'a Path,
    workspace_root: &'a Path,
    emit: fn(&LogEvent),
    silent: bool,
}

/// What the selection pre-pass settled: the tasks to run, the projects to
/// run them over, and the report recording the run.
struct PipelinePlan<'a, 'graph> {
    name: &'a str,
    requested_tasks: &'a [String],
    graph: &'a ProjectGraph<GraphPkg<'graph>>,
    selection: &'a Selection,
    report: &'a RunReport,
}

impl<'a> PipelineRun<'a> {
    fn info(&self, message: String) {
        (self.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message,
            prefix: self.workspace_root.to_string_lossy().into_owned(),
        }));
    }

    fn data_dir(&self) -> PathBuf {
        pipeline_data_dir(self.config, self.workspace_root)
    }

    /// The named pipeline's tasks, or the default pipeline's without a name.
    fn requested_tasks(&self) -> miette::Result<(&'a str, &'a [String])> {
        if self.config.pipelines.is_empty() {
            return Err(PipelineError::NoPipelines.into());
        }
        let name = self.invocation.name.as_deref().unwrap_or(DEFAULT_PIPELINE_NAME);
        let Some(requested_tasks) = self.config.pipelines.get(name) else {
            return Err(PipelineError::UnknownPipeline {
                name: name.to_string(),
                available: self.config.pipelines.keys().cloned().collect::<Vec<_>>().join(", "),
            }
            .into());
        };
        Ok((name, requested_tasks.as_slice()))
    }

    /// Run the plan's tasks, serving cached results where the keys match,
    /// and report how the run went.
    fn execute(&self, plan: &PipelinePlan<'_, '_>) -> miette::Result<PipelineOutcome> {
        let mut task_graph = self.task_graph(plan);
        let sequenced_tasks = sequence_tasks(
            &mut task_graph,
            &SequenceTasksOptions {
                workspace_dir: self.workspace_root,
                ignore_cycles: self.config.ignore_workspace_cycles,
                emit: self.emit,
            },
        )?;

        if self.invocation.dry_run {
            print_dry_run(self.invocation, &task_graph, &sequenced_tasks, self.workspace_root)?;
            return Ok(PipelineOutcome::without_upload());
        }

        let cache = TaskCache::open(&self.data_dir(), self.workspace_root)?;
        // Keys are computed for every task before anything runs, walking the
        // sequenced order so a task's dependency keys exist when its own is
        // built. This is also what a distributed tier would need: the whole
        // plan, priced, without executing. `--no-cache` skips the pricing
        // altogether: nothing reads a key, and hashing every tracked file of
        // every project is the bulk of what the flag exists to avoid.
        let task_keys = if self.invocation.no_cache {
            HashMap::new()
        } else {
            compute_task_keys(&task_graph, &sequenced_tasks, plan.graph, &cache, self.config)?
        };

        capture::install_forward(self.emit);
        let runner = TaskRunner {
            run: self,
            graph: plan.graph,
            cache: &cache,
            task_keys: &task_keys,
            report: plan.report,
            init_cwd: env::current_dir().unwrap_or_else(|_| self.dir.to_path_buf()),
            base_extra_env: self.config.extra_env_with_node_options(),
            statuses: Mutex::new(
                task_graph
                    .keys()
                    .map(|key| (format_task(key, self.workspace_root), ExecutionStatus::queued()))
                    .collect(),
            ),
            abort: Mutex::new(None),
        };
        schedule_tasks(
            &task_graph,
            &ScheduleTasksOptions {
                concurrency: usize::try_from(self.config.workspace_concurrency)
                    .unwrap_or(usize::MAX)
                    .max(1),
                bail: false,
                run_task: &|node: &TaskNode| runner.run_task(node),
                on_task_skipped: &|node: &TaskNode| runner.skip_task(node),
            },
        );

        let statuses = runner.finish()?;
        let counts = StatusCounts::of(&statuses);
        let hits = plan.report.cache_hits();

        plan.report.finish(&statuses, &task_keys, self.workspace_root);
        self.conclude(
            plan.report,
            Some(format!(
                r#"Pipeline "{}": {} tasks — {} passed ({hits} from cache), {} failed, {} skipped."#,
                plan.name,
                statuses.len(),
                counts.passed,
                counts.failed,
                counts.skipped,
            )),
            counts.failed,
        )
    }

    /// The task graph over the selection: the requested projects' tasks
    /// plus the `dependsOn` edges into the rest of the selected projects.
    fn task_graph(&self, plan: &PipelinePlan<'_, '_>) -> TaskGraph {
        let selected_graph: ProjectGraph<GraphPkg<'_>> = plan
            .graph
            .iter()
            .filter(|(root, _)| plan.selection.selected.contains(root.as_path()))
            .map(|(root, node)| (root.clone(), node.clone()))
            .collect();
        let project_dependencies =
            filtered_projects_dependencies(&selected_graph, plan.graph, None, &HashSet::new());

        let select_scripts = |project: &Path, task_name: &str| -> Vec<String> {
            let manifest = plan.graph[project].package.project.manifest.value();
            match ScriptSelector::new(task_name) {
                Ok(selector) => selector.select(manifest),
                Err(_) => Vec::new(),
            }
        };
        let task_names: Vec<&str> = plan.requested_tasks.iter().map(String::as_str).collect();
        build_pipeline_task_graph(&BuildPipelineTaskGraphOptions {
            project_dependencies: &project_dependencies,
            select_scripts,
            task_names: &task_names,
            requested_projects: Some(&plan.selection.requested),
            tasks: (!self.config.tasks.is_empty()).then_some(&self.config.tasks),
        })
    }

    /// Write the report and name it, after the summary line when there is
    /// one.
    fn conclude(
        &self,
        report: &RunReport,
        summary: Option<String>,
        failed_tasks: usize,
    ) -> miette::Result<PipelineOutcome> {
        let report_dir = report.write(&self.data_dir())?;
        if let Some(summary) = summary {
            self.info(summary);
        }
        self.info(format!("Report: {}", report_dir.display()));
        Ok(PipelineOutcome {
            failed_tasks,
            upload: Some(report.to_upload(workspace_identity(self.workspace_root))),
        })
    }
}

/// The per-task state the scheduler's callbacks read and update.
struct TaskRunner<'a, 'graph> {
    run: &'a PipelineRun<'a>,
    graph: &'a ProjectGraph<GraphPkg<'graph>>,
    cache: &'a TaskCache,
    task_keys: &'a HashMap<TaskKey, Option<String>>,
    report: &'a RunReport,
    init_cwd: PathBuf,
    base_extra_env: HashMap<String, String>,
    statuses: Mutex<IndexMap<String, ExecutionStatus>>,
    abort: Mutex<Option<miette::Report>>,
}

impl TaskRunner<'_, '_> {
    fn run_task(&self, node: &TaskNode) -> TaskCompletion {
        let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
        let summary_key = format_task(&key, self.run.workspace_root);
        let outcome = run_pipeline_task(&RunTaskOptions {
            node,
            graph: self.graph,
            config: self.run.config,
            invocation: self.run.invocation,
            cache: self.cache,
            task_key: self.task_keys.get(&key).and_then(Option::as_deref),
            init_cwd: &self.init_cwd,
            base_extra_env: &self.base_extra_env,
            emit: self.run.emit,
            silent: self.run.silent,
            report: self.report,
            summary_key: &summary_key,
        });
        record_task_outcome(&self.statuses, &self.abort, &summary_key, outcome)
    }

    fn skip_task(&self, node: &TaskNode) {
        let key = TaskKey { project: node.project.clone(), task_name: node.task_name.clone() };
        let summary_key = format_task(&key, self.run.workspace_root);
        self.statuses.lock().expect("status lock is not poisoned")[&summary_key].status =
            Status::Skipped;
        self.report.task_skipped(&summary_key);
    }

    /// The settled statuses, unless a task could not run at all.
    fn finish(self) -> miette::Result<IndexMap<String, ExecutionStatus>> {
        if let Some(error) = self.abort.into_inner().expect("abort slot lock is not poisoned") {
            return Err(error);
        }
        Ok(self.statuses.into_inner().expect("status lock is not poisoned"))
    }
}

struct StatusCounts {
    failed: usize,
    passed: usize,
    skipped: usize,
}

impl StatusCounts {
    fn of(statuses: &IndexMap<String, ExecutionStatus>) -> Self {
        let count =
            |wanted: Status| statuses.values().filter(|status| status.status == wanted).count();
        StatusCounts {
            failed: count(Status::Failure),
            passed: count(Status::Passed),
            skipped: count(Status::Skipped),
        }
    }
}

/// Where the pipeline keeps its task cache, restore records, and run
/// reports: under pnpm's cache directory, keyed by workspace path, so an
/// install pruning `node_modules` cannot take the cache with it.
fn pipeline_data_dir(config: &Config, workspace_root: &Path) -> PathBuf {
    let workspace_slug = pnpm_crypto_hash::create_short_hash(&workspace_root.to_string_lossy());
    config.cache_dir.join("pipeline").join(workspace_slug)
}

fn build_full_graph<'a>(
    projects: &'a [Project],
    config: &Config,
    workspace_root: &Path,
) -> miette::Result<ProjectGraph<GraphPkg<'a>>> {
    let catalogs = configured_catalogs(config)?;
    let dependency_rewriter = overrides_dependency_rewriter(config, &catalogs, workspace_root)
        .into_diagnostic()
        .wrap_err("parsing the overrides")?;
    let graph_options = CreateProjectsGraphOptions {
        link_workspace_packages: Some(
            config.link_workspace_packages != pnpm_config::LinkWorkspacePackages::Off,
        ),
        dependency_rewriter: dependency_rewriter
            .as_ref()
            .map(|rewriter| rewriter as &dyn DependencyRewriter),
        ..CreateProjectsGraphOptions::default()
    };
    Ok(create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &graph_options,
    )
    .graph)
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
/// `--dry-run` prints the plan instead of running it.
fn print_dry_run(
    invocation: &PipelineInvocation,
    task_graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    workspace_root: &Path,
) -> miette::Result<()> {
    if invocation.json {
        let document = task_graph_to_json(task_graph, workspace_root);
        println!("{}", serde_json::to_string_pretty(&document).into_diagnostic()?);
    } else {
        println!("{}", render_task_graph_dry_run(task_graph, sequenced_tasks, workspace_root));
    }
    Ok(())
}

/// Record one task's status. A task that could not run at all aborts the
/// whole pipeline; a task that ran and failed only fails itself, because
/// the pipeline never bails.
fn record_task_outcome(
    statuses: &Mutex<IndexMap<String, ExecutionStatus>>,
    abort: &Mutex<Option<miette::Report>>,
    summary_key: &str,
    outcome: miette::Result<ExecutionStatus>,
) -> TaskCompletion {
    let status = match outcome {
        Ok(status) => status,
        Err(error) => {
            let mut abort = abort.lock().expect("abort slot lock is not poisoned");
            if abort.is_none() {
                *abort = Some(error);
            }
            return TaskCompletion::Aborted;
        }
    };
    let failed = status.status == Status::Failure;
    statuses.lock().expect("status lock is not poisoned")[summary_key] = status;
    if failed { TaskCompletion::Failed } else { TaskCompletion::Passed }
}

fn select_affected_projects(options: &SelectAffectedOptions<'_>) -> miette::Result<Selection> {
    let all_dirs: Vec<PathBuf> = options
        .graph
        .keys()
        .filter(|dir| {
            options.config.include_workspace_root || dir.as_path() != options.workspace_root
        })
        .cloned()
        .collect();
    let full_selection = |merge_base: Option<String>, changed_count: usize| Selection {
        requested: all_dirs.clone(),
        selected: all_dirs.iter().cloned().collect(),
        mode: SelectionMode::Full,
        merge_base,
        changed_count,
    };

    if options.full {
        return Ok(full_selection(None, 0));
    }
    let Some(merge_base) = resolve_merge_base(options.workspace_root, options.base) else {
        (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                "Cannot resolve the merge base of HEAD and {}; running the pipeline over every project.",
                options.base,
            ),
            prefix: options.workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(None, 0));
    };

    let changed = get_changed_projects(
        options.graph.keys().cloned().collect(),
        &merge_base,
        &GetChangedProjectsOptions {
            workspace_dir: options.workspace_root,
            test_pattern: &options.config.test_pattern,
            changed_files_ignore_pattern: &options.config.changed_files_ignore_pattern,
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
        .any(|dir| dir == options.workspace_root)
    {
        (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message:
                "The diff touches workspace-root files; running the pipeline over every project."
                    .to_string(),
            prefix: options.workspace_root.to_string_lossy().into_owned(),
        }));
        return Ok(full_selection(Some(merge_base), changed_count));
    }

    let mut affected = projects_with_dependents(options.graph, &changed.changed_projects);
    // A project whose only changes match `testPattern` is selected itself
    // without pulling in its dependents.
    affected.extend(changed.ignore_dependent_for_projects.iter().cloned());
    if !options.config.include_workspace_root {
        affected.remove(options.workspace_root);
    }
    let selected = with_transitive_dependencies(
        options.graph,
        &affected,
        options.workspace_root,
        options.config,
    );

    // In the workspace graph's deterministic order, which is the
    // dispatch tie-break order.
    let requested: Vec<PathBuf> =
        options.graph.keys().filter(|dir| affected.contains(dir.as_path())).cloned().collect();
    Ok(Selection {
        requested,
        selected,
        mode: SelectionMode::Affected,
        merge_base: Some(merge_base),
        changed_count,
    })
}

/// The changed projects and everything that depends on them, directly or
/// not.
fn projects_with_dependents(
    graph: &ProjectGraph<GraphPkg<'_>>,
    changed_projects: &[PathBuf],
) -> HashSet<PathBuf> {
    let mut dependents: HashMap<&Path, Vec<&Path>> = HashMap::new();
    for (dir, node) in graph {
        for dependency in &node.dependencies {
            dependents.entry(dependency.as_path()).or_default().push(dir.as_path());
        }
    }
    let mut affected: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<&Path> = changed_projects.iter().map(PathBuf::as_path).collect();
    while let Some(dir) = stack.pop() {
        if !affected.insert(dir.to_path_buf()) {
            continue;
        }
        stack.extend(dependents.get(dir).into_iter().flatten());
    }
    affected
}

/// The task graph additionally spans the affected set's transitive
/// dependencies; see [`Selection::selected`] for why.
fn with_transitive_dependencies(
    graph: &ProjectGraph<GraphPkg<'_>>,
    affected: &HashSet<PathBuf>,
    workspace_root: &Path,
    config: &Config,
) -> HashSet<PathBuf> {
    let mut selected = affected.clone();
    let mut stack: Vec<PathBuf> = affected.iter().cloned().collect();
    while let Some(dir) = stack.pop() {
        let dependencies =
            graph.get(&dir).map(|node| node.dependencies.as_slice()).unwrap_or_default();
        for dependency in dependencies {
            if (config.include_workspace_root || dependency.as_path() != workspace_root)
                && selected.insert(dependency.clone())
            {
                stack.push(dependency.clone());
            }
        }
    }
    selected
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
) -> miette::Result<HashMap<TaskKey, Option<String>>> {
    let mut keys: HashMap<TaskKey, Option<String>> = HashMap::with_capacity(task_graph.len());
    for key in sequenced_tasks {
        let node = &task_graph[key];
        let manifest = graph[node.project.as_path()].package.project.manifest.value();
        let script_bodies = task_script_bodies(node, manifest, config.enable_pre_post_scripts);
        let Some(mut dependency_keys) = node
            .dependencies
            .iter()
            .map(|dependency| keys[dependency].as_deref())
            .collect::<Option<Vec<&str>>>()
        else {
            keys.insert(key.clone(), None);
            continue;
        };
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
    task_key: Option<&'a str>,
    init_cwd: &'a Path,
    base_extra_env: &'a HashMap<String, String>,
    emit: fn(&LogEvent),
    silent: bool,
    report: &'a RunReport,
    summary_key: &'a str,
}

/// Script failures are returned as statuses. Infrastructure errors abort the run.
fn run_pipeline_task(options: &RunTaskOptions<'_, '_>) -> miette::Result<ExecutionStatus> {
    let root = options.node.project.as_path();
    let summary_key = options.summary_key;
    let settings = options.config.tasks.get(&options.node.task_name);
    let cache_key = options.task_key.filter(|_| task_cacheable(options.invocation, settings));
    let start = Instant::now();
    options.report.task_started(summary_key, options.task_key);

    if let Some(cache_key) = cache_key
        && let Some(restored) = try_restore(options, cache_key, start)?
    {
        return Ok(restored);
    }

    let execution = execute_task_with_cargo_cache(options, settings)?;
    let duration = start.elapsed().as_secs_f64() * 1e3;
    if execution.status == Status::Passed
        && let Some(cache_key) = cache_key
        && let Some(captured) = execution.captured
    {
        let outputs = settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default();
        if let Err(error) = options.cache.store(cache_key, root, summary_key, outputs, captured) {
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("{summary_key}: failed to store the task in the cache: {error}"),
                prefix: root.to_string_lossy().into_owned(),
            }));
        }
    }
    let disposition =
        if cache_key.is_some() { CacheDisposition::Miss } else { CacheDisposition::Bypass };
    options.report.task_finished(summary_key, execution.status, disposition, duration);
    Ok(ExecutionStatus {
        status: execution.status,
        duration: Some(duration),
        prefix: (execution.status == Status::Failure).then(|| root.to_string_lossy().into_owned()),
        message: execution.message,
    })
}

/// The status of a task served from the cache, or `None` when nothing is
/// stored under `cache_key` or the restore refused.
fn try_restore(
    options: &RunTaskOptions<'_, '_>,
    cache_key: &str,
    start: Instant,
) -> miette::Result<Option<ExecutionStatus>> {
    let root = options.node.project.as_path();
    let summary_key = options.summary_key;
    let Some(stored) = options.cache.lookup(cache_key) else {
        return Ok(None);
    };
    match options.cache.restore(&stored, root, summary_key) {
        Ok(()) => {
            capture::replay(&stored.scripts, root, options.emit);
            sync_injected_deps_if_configured(options.config, options.node, options.graph)?;
            let duration = start.elapsed().as_secs_f64() * 1e3;
            options.report.task_finished(
                summary_key,
                Status::Passed,
                CacheDisposition::Hit,
                duration,
            );
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: format!("{summary_key}: restored from cache"),
                prefix: root.to_string_lossy().into_owned(),
            }));
            Ok(Some(ExecutionStatus {
                status: Status::Passed,
                duration: Some(duration),
                prefix: None,
                message: None,
            }))
        }
        Err(reason) => {
            // A file the restore cannot account for is the user's;
            // overwriting it silently is how caches lose trust. The
            // task runs normally instead.
            (options.emit)(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!("{summary_key}: not restoring from cache: {reason}"),
                prefix: root.to_string_lossy().into_owned(),
            }));
            Ok(None)
        }
    }
}

fn execute_task_with_cargo_cache(
    options: &RunTaskOptions<'_, '_>,
    settings: Option<&pnpm_config::TaskSettings>,
) -> miette::Result<TaskExecution> {
    let root = options.node.project.as_path();
    let Some(directory) = settings.and_then(|settings| settings.cargo_target_dir.as_deref()) else {
        return execute_task_scripts(options);
    };
    let cargo = cargo_cache::CargoCache::open(root, directory).into_diagnostic()?;
    let environment = cargo_cache::cache_environment(
        options.base_extra_env,
        settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default(),
    );
    let cargo_cacheable = !options.invocation.no_cache
        && settings.is_some_and(|settings| settings.cache != Some(false));
    let snapshot = cargo_cacheable.then_some(options.task_key).flatten().and_then(|task_key| {
        cargo_cache::snapshot_entry(&options.config.cache_dir, root, task_key, &environment)
            .inspect_err(|error| cargo_cache_warning(options, &error.to_string()))
            .ok()
    });
    if let Some(snapshot) = &snapshot {
        restore_cargo_snapshot(options, &cargo, snapshot)?;
    }
    let extra_env = cargo_build_env(options.base_extra_env, &cargo);
    let execution =
        execute_task_scripts(&RunTaskOptions { base_extra_env: &extra_env, ..*options })?;
    if execution.status == Status::Passed
        && let Some(task_key) = options.task_key
        && let Some(snapshot) = &snapshot
    {
        publish_cargo_snapshot(options, &cargo, snapshot, task_key, &environment);
    }
    Ok(execution)
}

/// Restore the key's snapshot into the build directory, then ready the
/// directory for this key's run.
fn restore_cargo_snapshot(
    options: &RunTaskOptions<'_, '_>,
    cargo: &cargo_cache::CargoCache,
    (entry, key, _): &(PathBuf, String, Vec<String>),
) -> miette::Result<()> {
    match cargo.restore(entry, key) {
        Ok(true) => (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: format!("{}: restored Cargo build state", options.summary_key),
            prefix: options.node.project.to_string_lossy().into_owned(),
        })),
        // A snapshot that is not there yet is the ordinary first run.
        Ok(false) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => cargo_cache_warning(options, &error.to_string()),
    }
    cargo.prepare(key).into_diagnostic()
}

/// The task's environment with Cargo pointed at the cached build directory.
fn cargo_build_env(
    base_extra_env: &HashMap<String, String>,
    cargo: &cargo_cache::CargoCache,
) -> HashMap<String, String> {
    let mut extra_env = base_extra_env.clone();
    for name in ["CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR"] {
        extra_env.insert(name.to_string(), cargo.target.to_string_lossy().into_owned());
    }
    extra_env
}

/// Save the build directory as this task key's snapshot — but only when
/// the inputs still hash to the key the run started with, since a
/// concurrent edit would otherwise be published under the wrong key.
fn publish_cargo_snapshot(
    options: &RunTaskOptions<'_, '_>,
    cargo: &cargo_cache::CargoCache,
    (entry, key, local_packages): &(PathBuf, String, Vec<String>),
    task_key: &str,
    environment: &std::collections::BTreeMap<String, String>,
) {
    let root = options.node.project.as_path();
    match cargo_cache::snapshot_entry(&options.config.cache_dir, root, task_key, environment) {
        Ok((_, after, _)) if after == *key => {
            if let Err(error) = cargo.publish(entry, key, local_packages) {
                cargo_cache_warning(options, &error.to_string());
            }
        }
        Ok(_) => {
            cargo_cache_warning(options, "inputs changed during execution; snapshot was not saved");
        }
        Err(error) => cargo_cache_warning(options, &error.to_string()),
    }
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
    let root = options.node.project.as_path();
    let manifest = &options.graph[root].package.project.manifest;

    let extra_env = task_environment(options.config, root, options.base_extra_env);
    let capture_output = options.task_key.is_some()
        && task_cacheable(options.invocation, options.config.tasks.get(&options.node.task_name));
    let root_str = root.to_string_lossy().into_owned();
    let mut execution = TaskExecution {
        status: Status::Passed,
        message: None,
        captured: capture_output.then(Vec::new),
    };
    let mut captured_bytes = 0usize;
    for selected in &options.node.scripts {
        let Some(script) = runnable_script(manifest, selected, root)? else {
            continue;
        };
        let ctx = RunContext {
            manifest,
            dir: root,
            init_cwd: options.init_cwd,
            config: options.config,
            extra_env: &extra_env,
            silent: options.silent,
            output: ScriptOutput::Streamed {
                dep_path: &root_str,
                emit: if capture_output { capture::capturing_emit } else { options.emit },
            },
            // The pipeline never bails, so there is no cancellation to
            // propagate into running children.
            process_tracker: None,
        };
        let exit = run_stages(&ctx, selected, &script, &[]);
        if capture_output {
            drain_captured_output(
                &mut execution.captured,
                &mut captured_bytes,
                &root_str,
                selected,
                options.config.enable_pre_post_scripts,
            );
        }
        let exit = exit?;
        if !exit.success() {
            execution.status = Status::Failure;
            execution.message =
                Some(format!("command failed with exit code {}", exit.code().unwrap_or(1)));
            break;
        }
    }
    Ok(execution)
}

/// Take the output one script produced into the capture buffer. A task
/// whose output outgrows the cap is not cached at all: a truncated log
/// would replay as a complete one.
fn drain_captured_output(
    captured: &mut Option<Vec<capture::CapturedScript>>,
    captured_bytes: &mut usize,
    root_str: &str,
    selected: &str,
    enable_pre_post_scripts: bool,
) {
    let Some(stages) = capture::drain_task(root_str, selected, enable_pre_post_scripts) else {
        *captured = None;
        return;
    };
    *captured_bytes += stages
        .iter()
        .flat_map(|stage| &stage.lines)
        .map(|line| line.line.len() + std::mem::size_of::<capture::CapturedLine>())
        .sum::<usize>();
    if *captured_bytes > capture::MAX_CAPTURE_BYTES {
        *captured = None;
    }
    if let Some(captured) = captured.as_mut() {
        captured.extend(stages);
    }
}

/// The script body to run for one selected script name. `None` when the
/// project declares none, when it is empty or the `only-allow` guard, or
/// when running it would re-enter the script pnpm is already inside.
fn runnable_script(
    manifest: &pnpm_package_manifest::PackageManifest,
    selected: &str,
    root: &Path,
) -> miette::Result<Option<String>> {
    let Some(script) = manifest.script(selected, true).map_err(miette::Report::new)? else {
        return Ok(None);
    };
    if script.is_empty() || script == "npx only-allow pnpm" {
        return Ok(None);
    }
    if env::var_os("npm_lifecycle_event").is_some_and(|event| event == *selected)
        && env::var_os("PNPM_SCRIPT_SRC_DIR").is_some_and(|src_dir| Path::new(&src_dir) == root)
    {
        return Ok(None);
    }
    Ok(Some(script.to_owned()))
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
