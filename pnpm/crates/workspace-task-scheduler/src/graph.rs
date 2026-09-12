use super::{
    HashMap, HashSet, LogEvent, LogLevel, Path, PnpmLog, Serialize, TaskCycle, TaskGraph, TaskKey,
    TaskNode, graph_sequencer,
};

pub struct SequenceTasksOptions<'a> {
    pub workspace_dir: &'a Path,
    /// The `ignoreWorkspaceCycles` setting: the workspace has declared its
    /// cycles deliberate, so a cyclic task graph is downgraded from an
    /// error to a warning, backward edges are dropped, and the members run
    /// in the graph sequencer's deterministic order.
    pub ignore_cycles: bool,
    pub emit: fn(&LogEvent),
}

/// Topologically order the task graph, erroring when the tasks form a cycle
/// unless `ignore_cycles` tolerates it and rewrites the graph's edges
/// acyclic. Detection is scoped to this graph: a cycle among tasks the
/// filter did not select cannot fail the run.
pub fn sequence_tasks(
    graph: &mut TaskGraph,
    options: &SequenceTasksOptions<'_>,
) -> Result<Vec<TaskKey>, TaskCycle> {
    let edges: HashMap<TaskKey, Vec<TaskKey>> =
        graph.iter().map(|(key, node)| (key.clone(), node.dependencies.clone())).collect();
    let included: Vec<TaskKey> = graph.keys().cloned().collect();
    let result = graph_sequencer(&edges, &included);
    if !result.cycles.is_empty() {
        let cycles = result
            .cycles
            .iter()
            .map(|cycle| {
                cycle
                    .iter()
                    .chain(cycle.first())
                    .map(|key| format_task(key, options.workspace_dir))
                    .collect::<Vec<_>>()
                    .join(" → ")
            })
            .collect::<Vec<_>>()
            .join("; ");
        if !options.ignore_cycles {
            return Err(TaskCycle { cycles });
        }
        (options.emit)(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                "The tasks form a dependency cycle and run in an arbitrary order relative to each other because ignoreWorkspaceCycles is set: {cycles}",
            ),
            prefix: options.workspace_dir.to_string_lossy().into_owned(),
        }));
        drop_cyclic_dependencies(graph, &result.order);
    }
    Ok(result.order)
}

/// Keep only dependencies that point backward in the sequencer's order,
/// making an ignored cyclic graph deterministic and runnable.
fn drop_cyclic_dependencies(graph: &mut TaskGraph, order: &[TaskKey]) {
    let order_index: HashMap<&TaskKey, usize> =
        order.iter().enumerate().map(|(index, key)| (key, index)).collect();
    let filtered: Vec<(TaskKey, Vec<TaskKey>)> = graph
        .iter()
        .map(|(key, node)| {
            (
                key.clone(),
                node.dependencies
                    .iter()
                    .filter(|dependency| order_index[*dependency] < order_index[key])
                    .cloned()
                    .collect(),
            )
        })
        .collect();
    for (key, dependencies) in filtered {
        graph[&key].dependencies = dependencies;
    }
}

/// `<workspace-relative dir>#<task name>`, with forward slashes on every
/// platform — the rendering of a task in cycle errors and dry-run output.
#[must_use]
pub fn format_task(key: &TaskKey, workspace_dir: &Path) -> String {
    format!("{}#{}", relative_project_dir(&key.project, workspace_dir), key.task_name)
}

fn relative_project_dir(project: &Path, workspace_dir: &Path) -> String {
    let relative = pnpm_fs::relative_path(workspace_dir, project);
    if relative == project {
        // The two could not be related (a different drive); the absolute
        // path is the only faithful rendering.
        return relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    }
    if relative.as_os_str().is_empty() {
        return ".".to_string();
    }
    relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// The same graph with every edge turned around: dependents run before
/// dependencies.
#[must_use]
pub fn reverse_task_graph(graph: &TaskGraph) -> TaskGraph {
    let mut reversed: TaskGraph = graph
        .iter()
        .map(|(key, node)| (key.clone(), TaskNode { dependencies: Vec::new(), ..node.clone() }))
        .collect();
    for (key, node) in graph {
        for dependency in &node.dependencies {
            reversed[dependency].dependencies.push(key.clone());
        }
    }
    reversed
}

/// The graph without the anchor's transitive dependencies — the tasks known
/// to have finished before a run would reach the anchor. Everything else
/// stays, including work unrelated to the anchor, and edges into the
/// dropped set are treated as satisfied.
#[must_use]
pub fn resume_task_graph_from(
    graph: TaskGraph,
    anchor_project: &Path,
    task_name: &str,
    completed_tasks: Option<&HashSet<TaskKey>>,
) -> TaskGraph {
    let anchor =
        TaskKey { project: anchor_project.to_path_buf(), task_name: task_name.to_string() };
    let Some(anchor_node) = graph.get(&anchor) else {
        // The anchor exists but its task is not in this graph: there is
        // nothing to skip.
        return graph;
    };
    let dropped = completed_tasks.map_or_else(
        || transitive_dependencies(&graph, anchor_node),
        |completed| {
            completed
                .iter()
                .filter(|key| **key != anchor && graph.contains_key(*key))
                .cloned()
                .collect()
        },
    );
    graph
        .into_iter()
        .filter(|(key, _)| !dropped.contains(key))
        .map(|(key, mut node)| {
            node.dependencies.retain(|dependency| !dropped.contains(dependency));
            (key, node)
        })
        .collect()
}

fn transitive_dependencies(graph: &TaskGraph, anchor: &TaskNode) -> HashSet<TaskKey> {
    let mut dependencies: HashSet<TaskKey> = HashSet::new();
    let mut stack: Vec<TaskKey> = anchor.dependencies.clone();
    while let Some(key) = stack.pop() {
        if !dependencies.insert(key.clone()) {
            continue;
        }
        stack.extend(graph[&key].dependencies.iter().cloned());
    }
    dependencies
}

/// Whether at most one script can ever be in flight, which is when output
/// may stay inherited rather than piped: no task runs several scripts, and
/// the scripts are held apart either by the dependency edges — every
/// script-running task on one chain — or by the per-task concurrency
/// limits [`schedule_tasks`](crate::synchronous::schedule_tasks) enforces.
///
/// `sequenced_tasks` is [`sequence_tasks`]'s result — the proof the graph
/// is acyclic, and the evaluation order for the longest-chain scan.
#[must_use]
pub fn is_serial_task_graph(graph: &TaskGraph, sequenced_tasks: &[TaskKey]) -> bool {
    let mut script_task_count = 0_usize;
    for node in graph.values() {
        if node.scripts.len() > 1 {
            return false;
        }
        script_task_count += node.scripts.len();
    }
    if script_task_count <= 1 || serialized_by_one_task_limit(graph) {
        return true;
    }
    let mut chain_length: HashMap<&TaskKey, usize> = HashMap::new();
    let mut longest_chain = 0_usize;
    for key in sequenced_tasks {
        let node = &graph[key];
        let via_dependencies = node
            .dependencies
            .iter()
            .map(|dependency| chain_length.get(dependency).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let length = via_dependencies + node.scripts.len();
        chain_length.insert(graph.get_key_value(key).expect("sequenced key is in graph").0, length);
        longest_chain = longest_chain.max(length);
    }
    longest_chain == script_task_count
}

/// Whether [`schedule_tasks`](crate::synchronous::schedule_tasks)'s concurrency limits alone leave at most one
/// script in flight: every script-running task shares a single limit group
/// — the group is the task name — and that group admits one task at a time.
fn serialized_by_one_task_limit(graph: &TaskGraph) -> bool {
    let mut limited_group: Option<&str> = None;
    for node in graph.values().filter(|node| !node.scripts.is_empty()) {
        // `schedule_tasks` floors the declared limit at 1.
        if node.concurrency.map(|limit| limit.max(1)) != Some(1) {
            return false;
        }
        let group = limited_group.get_or_insert(node.task_name.as_str());
        if *group != node.task_name.as_str() {
            return false;
        }
    }
    true
}

/// The task's key in the recursive summary. The task of the script the
/// invocation named keeps the project directory alone — the format
/// existing consumers of `pnpm-exec-summary.json` read — and only tasks
/// `dependsOn` pulled in qualify it with the task name.
#[must_use]
pub fn task_summary_key(node: &TaskNode) -> String {
    if node.requested {
        node.project.to_string_lossy().into_owned()
    } else {
        format!("{}#{}", node.project.to_string_lossy(), node.task_name)
    }
}

/// One task reference in the `--dry-run --json` output.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DryRunTaskDependency {
    pub project: String,
    pub script: String,
}

/// One task in the `--dry-run --json` output.
#[derive(Debug, Serialize)]
pub struct DryRunTask {
    pub project: String,
    pub script: String,
    #[serde(rename = "missingScript")]
    pub missing_script: bool,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<DryRunTaskDependency>,
}

/// The `--dry-run --json` document: `{ "tasks": [...] }`.
#[derive(Debug, Serialize)]
pub struct DryRunDocument {
    pub tasks: Vec<DryRunTask>,
}

/// What `--dry-run --json` emits: nodes and edges rather than an order,
/// since independent tasks have no required sequence. Identifiers are the
/// workspace-relative project directory and the script name.
#[must_use]
pub fn task_graph_to_json(graph: &TaskGraph, workspace_dir: &Path) -> DryRunDocument {
    let mut tasks: Vec<DryRunTask> = graph
        .values()
        .map(|node| {
            let mut depends_on: Vec<DryRunTaskDependency> = node
                .dependencies
                .iter()
                .map(|dependency| DryRunTaskDependency {
                    project: relative_project_dir(&dependency.project, workspace_dir),
                    script: dependency.task_name.clone(),
                })
                .collect();
            depends_on.sort();
            DryRunTask {
                project: relative_project_dir(&node.project, workspace_dir),
                script: node.task_name.clone(),
                missing_script: node.scripts.is_empty(),
                depends_on,
            }
        })
        .collect();
    tasks.sort_by(|left, right| {
        left.project.cmp(&right.project).then_with(|| left.script.cmp(&right.script))
    });
    DryRunDocument { tasks }
}

/// What plain `--dry-run` prints: one valid linearization of the graph —
/// not the order the scheduler will follow. Ties among simultaneously
/// runnable tasks are broken by project directory, so two dry runs of one
/// workspace print the same thing and their diff is meaningful.
#[must_use]
pub fn render_task_graph_dry_run(
    graph: &TaskGraph,
    sequenced_tasks: &[TaskKey],
    workspace_dir: &Path,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for key in sequenced_tasks {
        let task = format_task(key, workspace_dir);
        lines.push(if graph[key].scripts.is_empty() {
            format!("{task} (skipped: no such script)")
        } else {
            task
        });
    }
    lines.join("\n")
}
