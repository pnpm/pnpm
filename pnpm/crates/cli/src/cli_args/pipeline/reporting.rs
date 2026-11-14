use super::{
    Config, ExecutionStatus, GraphPkg, HashMap, HashSet, IndexMap, IntoDiagnostic, Mutex, Path,
    PathBuf, PipelineInvocation, PipelineOutcome, PipelineResults, PipelineRun, ProjectGraph,
    RunReport, SelectionMode, Status, TaskCache, TaskCompletion, TaskGraph, TaskKey, TaskNode,
    Value, cache, format_task, is_output_cacheable, render_task_graph_dry_run, task_environment,
    task_graph_to_json,
};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};

pub(super) struct StatusCounts {
    pub(super) failed: usize,
    pub(super) passed: usize,
    pub(super) skipped: usize,
}

impl StatusCounts {
    pub(super) fn of(statuses: &IndexMap<String, ExecutionStatus>) -> Self {
        let count = |wanted: Status| {
            statuses
                .values()
                .filter(|status| status.status == wanted)
                .count()
        };
        StatusCounts {
            failed: count(Status::Failure),
            passed: count(Status::Passed),
            skipped: count(Status::Skipped),
        }
    }
}

/// The selection pre-pass: changed projects since the merge base, plus
/// their transitive dependents. It is an optimization, not the
/// correctness boundary — any doubt about attribution (the merge base
/// cannot be resolved, or the diff touches the workspace root, whose
/// files feed every project) falls through to the full graph.
/// `--dry-run` prints the plan instead of running it.
pub(super) fn print_dry_run(
    invocation: &PipelineInvocation,
    task_graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    workspace_root: &Path,
) -> miette::Result<()> {
    if invocation.json {
        let document = task_graph_to_json(task_graph, workspace_root);
        pnpm_diagnostics::println!(
            "{}",
            serde_json::to_string_pretty(&document).into_diagnostic()?,
        );
    } else {
        pnpm_diagnostics::println!(
            "{}",
            render_task_graph_dry_run(task_graph, sequenced_tasks, workspace_root),
        );
    }
    Ok(())
}

impl PipelineRun<'_> {
    /// Nothing is selected, so nothing runs. A dry run writes no report, and
    /// with `--json` prints the empty plan.
    pub(super) fn conclude_empty_selection(
        &self,
        report: &RunReport,
        mode: SelectionMode,
        base: &str,
    ) -> miette::Result<PipelineOutcome> {
        if self.invocation.dry_run && self.invocation.json {
            print_dry_run(self.invocation, &TaskGraph::default(), &[], self.workspace_root)?;
            return Ok(PipelineOutcome::without_upload());
        }
        self.info(empty_selection_message(mode, base));
        if self.invocation.dry_run {
            return Ok(PipelineOutcome::without_upload());
        }
        self.conclude(report, None, 0)
    }
}

fn empty_selection_message(mode: SelectionMode, base: &str) -> String {
    match mode {
        SelectionMode::Affected => {
            format!("No projects are affected since {base} — nothing to run.")
        }
        SelectionMode::Full => "No projects are selected — nothing to run.".to_string(),
    }
}

/// Record one task's status. A task that could not run at all aborts the
/// whole pipeline; a task that ran and failed only fails itself, because
/// the pipeline never bails.
pub(super) fn record_task_outcome(
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

pub(crate) struct TaskCachePlan {
    pub(super) task_key: Option<String>,
    pub(super) output_cache_key: Option<String>,
    upstream_outputs_cached: bool,
}

pub(super) fn reported_task_keys(
    plans: &HashMap<TaskKey, TaskCachePlan>,
) -> HashMap<TaskKey, Option<String>> {
    plans
        .iter()
        .map(|(task, plan)| (task.clone(), plan.task_key.clone()))
        .collect()
}

struct KeyPlanContext<'a, 'graph> {
    task_graph: &'a TaskGraph,
    graph: &'a ProjectGraph<GraphPkg<'graph>>,
    cache: &'a TaskCache,
    config: &'a Config,
    lockfile_selection: pnpm_lockfile::WantedLockfileSelection,
    base_extra_env: HashMap<String, String>,
}

/// Pass-through tasks contribute keys to invalidate their dependents.
pub(super) fn compute_task_keys(
    task_graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    graph: &ProjectGraph<GraphPkg<'_>>,
    cache: &TaskCache,
    config: &Config,
    emit: fn(&LogEvent),
) -> miette::Result<HashMap<TaskKey, TaskCachePlan>> {
    let ctx = KeyPlanContext {
        task_graph,
        graph,
        cache,
        config,
        lockfile_selection: config.wanted_lockfile_selection(),
        base_extra_env: config.extra_env_with_node_options(),
    };
    let mut plans: HashMap<TaskKey, TaskCachePlan> = HashMap::with_capacity(task_graph.len());
    let mut warned_projects: HashSet<PathBuf> = HashSet::new();
    for key in sequenced_tasks {
        let node = &task_graph[key];
        let plan = compute_single_task_plan(&ctx, node, &plans)?;
        if plan.task_key.is_none() {
            warn_without_git_inputs(cache, &node.project, &mut warned_projects, emit);
        }
        plans.insert(key.clone(), plan);
    }
    Ok(plans)
}

fn compute_single_task_plan(
    ctx: &KeyPlanContext<'_, '_>,
    node: &TaskNode,
    plans: &HashMap<TaskKey, TaskCachePlan>,
) -> miette::Result<TaskCachePlan> {
    let upstream_outputs_cached =
        upstream_outputs_are_cached(ctx.task_graph, node, plans, ctx.config);
    let Some(mut dependency_keys) = node.dependencies
        .iter()
        .map(|dependency| plans[dependency].task_key.as_deref())
        .collect::<Option<Vec<&str>>>()
    else {
        return Ok(TaskCachePlan {
            task_key: None,
            output_cache_key: None,
            upstream_outputs_cached,
        });
    };
    dependency_keys.sort_unstable();
    let settings = ctx.config.tasks.get(&node.task_name);
    let manifest = ctx.graph[node.project.as_path()].package.project.manifest.value();
    let script_bodies = task_script_bodies(node, manifest, ctx.config.enable_pre_post_scripts);
    let task_key = ctx.cache.compute_task_key(&cache::TaskKeyInputs {
        node,
        settings,
        lockfile_dir: ctx.config.lockfile_dir_for(&node.project),
        lockfile_selection: &ctx.lockfile_selection,
        dependency_keys: &dependency_keys,
        script_bodies: &script_bodies,
        environment: &task_environment(ctx.config, &node.project, &ctx.base_extra_env),
    })?;
    let output_cache_key = task_key
        .clone()
        .filter(|_| upstream_outputs_cached && is_output_cacheable(settings));
    Ok(TaskCachePlan { task_key, output_cache_key, upstream_outputs_cached })
}

/// Executable tasks without output caching (`cache: false`, omitted
/// `outputs`, or `cargoTargetDir`) run every time and may rewrite files
/// on disk, so their dependents keep input keys (for Cargo snapshots and
/// run reports) while bypassing output-cache hits.
fn upstream_outputs_are_cached(
    task_graph: &TaskGraph,
    node: &TaskNode,
    plans: &HashMap<TaskKey, TaskCachePlan>,
    config: &Config,
) -> bool {
    node.dependencies
        .iter()
        .all(|dependency| {
            let dependency_node = &task_graph[dependency];
            let dependency_cached = dependency_node.scripts.is_empty()
                || is_output_cacheable(config.tasks.get(&dependency_node.task_name));
            dependency_cached && plans[dependency].upstream_outputs_cached
        })
}

/// Explain a task that runs without a cache key because git cannot
/// enumerate its project, once per project.
fn warn_without_git_inputs(
    cache: &TaskCache,
    project: &Path,
    warned_projects: &mut HashSet<PathBuf>,
    emit: fn(&LogEvent),
) {
    let unavailable = cache.inputs_unavailable(project);
    if !matches!(unavailable, Some(cache::InputsUnavailable::NoGit))
        || !warned_projects.insert(project.to_path_buf())
    {
        return;
    }
    emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: "Cannot enumerate the tracked files of the project with git; running its tasks \
                  without a cache key."
            .to_string(),
        prefix: project.to_string_lossy().into_owned(),
    }));
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

impl PipelineResults {
    pub(super) fn new(graph: &TaskGraph, workspace_root: &Path) -> Self {
        Self {
            statuses: Mutex::new(
                graph
                    .keys()
                    .map(|key| (format_task(key, workspace_root), ExecutionStatus::queued()))
                    .collect(),
            ),
            abort: Mutex::new(None),
        }
    }
}
