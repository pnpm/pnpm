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
pub use selection::{Selection, SelectionMode};

use super::{
    install::InstallArgs,
    recursive::{ExecutionStatus, Status, discover_workspace_projects},
    reporter::{ReporterType, reporter_emit},
    run::{RunContext, ScriptSelector, run_stages},
};
use crate::cli_args::recursive::filtered_projects_dependencies;

use cache::{CacheDisposition, TaskCache};
use clap::Args;
use derive_more::{Display, Error};
use execution::{RunTaskOptions, run_pipeline_task, task_environment};
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
use report::RunReport;

use reporting::{StatusCounts, compute_task_keys, print_dry_run, record_task_outcome};
use selection::{
    SelectAffectedOptions, build_full_graph, git_stdout, select_affected_projects,
    workspace_identity,
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
    let graph = build_full_graph(&projects, config);

    let base = pipeline_base(invocation, config);
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
        runner.schedule(&task_graph);

        let statuses = runner.finish()?;
        self.finish_plan(plan, &statuses, &task_keys)
    }

    fn finish_plan(
        &self,
        plan: &PipelinePlan<'_, '_>,
        statuses: &IndexMap<String, ExecutionStatus>,
        task_keys: &HashMap<TaskKey, Option<String>>,
    ) -> miette::Result<PipelineOutcome> {
        let counts = StatusCounts::of(statuses);
        let hits = plan.report.cache_hits();

        plan.report.finish(statuses, task_keys, self.workspace_root);
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
    fn schedule(&self, task_graph: &TaskGraph) {
        schedule_tasks(
            task_graph,
            &ScheduleTasksOptions {
                concurrency: usize::try_from(self.run.config.workspace_concurrency)
                    .unwrap_or(usize::MAX)
                    .max(1),
                bail: false,
                run_task: &|node: &TaskNode| self.run_task(node),
                on_task_skipped: &|node: &TaskNode| self.skip_task(node),
            },
        );
    }

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

/// Where the pipeline keeps its task cache, restore records, and run
/// reports: under pnpm's cache directory, keyed by workspace path, so an
/// install pruning `node_modules` cannot take the cache with it.
fn pipeline_data_dir(config: &Config, workspace_root: &Path) -> PathBuf {
    let workspace_slug = pnpm_crypto_hash::create_short_hash(&workspace_root.to_string_lossy());
    config.cache_dir.join("pipeline").join(workspace_slug)
}

fn pipeline_base(invocation: &PipelineInvocation, config: &Config) -> String {
    invocation
        .base
        .clone()
        .or_else(|| config.pipeline_base.clone())
        .unwrap_or_else(|| DEFAULT_PIPELINE_BASE.to_string())
}

mod selection;

mod execution;

mod reporting;
