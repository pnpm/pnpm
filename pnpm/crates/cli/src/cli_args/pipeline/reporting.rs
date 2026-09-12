use super::{
    Config, ExecutionStatus, GraphPkg, HashMap, IndexMap, IntoDiagnostic, Mutex, Path,
    PipelineInvocation, ProjectGraph, Status, TaskCache, TaskCompletion, TaskGraph, TaskKey,
    TaskNode, Value, cache, render_task_graph_dry_run, task_environment, task_graph_to_json,
};

pub(super) struct StatusCounts {
    pub(super) failed: usize,
    pub(super) passed: usize,
    pub(super) skipped: usize,
}

impl StatusCounts {
    pub(super) fn of(statuses: &IndexMap<String, ExecutionStatus>) -> Self {
        let count =
            |wanted: Status| statuses.values().filter(|status| status.status == wanted).count();
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
        println!("{}", serde_json::to_string_pretty(&document).into_diagnostic()?);
    } else {
        println!("{}", render_task_graph_dry_run(task_graph, sequenced_tasks, workspace_root));
    }
    Ok(())
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

/// Pass-through tasks contribute keys to invalidate their dependents.
pub(super) fn compute_task_keys(
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
