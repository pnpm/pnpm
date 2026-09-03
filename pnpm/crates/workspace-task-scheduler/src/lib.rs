//! The task graph of one recursive `run` / `exec` invocation, and the
//! scheduler that dispatches it.
//!
//! A task is a `(project, task name)` pair. A task becomes runnable when
//! every task it depends on has completed successfully; runnable tasks are
//! dispatched under the `workspaceConcurrency` limit, with no barrier
//! between dependency-independent tasks. Mirrors `taskGraph.ts` /
//! `taskScheduler.ts` in pnpm's `@pnpm/workspace.task-scheduler`.

use derive_more::{Display, Error};
use futures_util::{StreamExt, stream::FuturesUnordered};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_config::TaskSettings;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
};

mod graph_sequencer;
pub use graph_sequencer::{GraphSequencerResult, PathNode, graph_sequencer};

/// The stable identifier of a task: the project directory and the task
/// (script) name. The scheduler, the summary, and the dry-run output agree
/// on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskKey {
    pub project: PathBuf,
    pub task_name: String,
}

#[derive(Debug, Clone)]
pub struct TaskNode {
    pub project: PathBuf,
    pub task_name: String,
    pub concurrency: Option<usize>,
    /// The scripts of the project that the task name selected — several
    /// when the task name is a `RegExp` selector. Empty when the project has
    /// no such script: the task is then a pass-through that runs nothing,
    /// completes as soon as its dependencies have, and is reported as
    /// skipped, so that a scriptless project does not sever a dependency
    /// chain.
    pub scripts: Vec<String>,
    /// Whether the invocation named this task, as opposed to `dependsOn`
    /// pulling it in.
    pub requested: bool,
    pub dependencies: Vec<TaskKey>,
}

/// Insertion order is the dispatch tie-break order, so it must stay the
/// selection order the project graph established.
pub type TaskGraph = IndexMap<TaskKey, TaskNode>;

/// `The tasks form a dependency cycle` — a cyclic task graph cannot be
/// scheduled, and running it in an arbitrary order would succeed or fail by
/// luck.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("The tasks form a dependency cycle: {cycles}")]
#[diagnostic(
    code(ERR_PNPM_TASK_CYCLE),
    help(
        "If the cycles are deliberate, set ignoreWorkspaceCycles to true to run their tasks in an arbitrary order."
    )
)]
pub struct TaskCycle {
    #[error(not(source))]
    pub cycles: String,
}

pub struct BuildTaskGraphOptions<'a, SelectScripts>
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    /// The dependency edges among the selected projects, already resolved
    /// through the full workspace graph. Tasks are created only for these
    /// projects: `dependsOn` never runs anything in a project the filter
    /// did not select.
    pub project_dependencies: &'a IndexMap<PathBuf, Vec<PathBuf>>,
    pub select_scripts: SelectScripts,
    /// The script the invocation runs; every selected project gets a task
    /// named this.
    pub task_name: &'a str,
    pub tasks: Option<&'a IndexMap<String, TaskSettings>>,
}

/// Build the graph of tasks the invocation runs: a task named `task_name`
/// in every selected project, plus every task those transitively pull in
/// through `dependsOn`. A task with no `tasks` entry behaves as
/// `dependsOn: ['^<its own name>']`: plain topological order over the
/// project graph.
pub fn build_task_graph<SelectScripts>(
    options: &BuildTaskGraphOptions<'_, SelectScripts>,
) -> TaskGraph
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    let mut graph: TaskGraph = IndexMap::new();
    let mut queue: VecDeque<(PathBuf, String, bool)> = options
        .project_dependencies
        .keys()
        .map(|project| (project.clone(), options.task_name.to_string(), true))
        .collect();
    while let Some((project, task_name, requested)) = queue.pop_front() {
        let key = TaskKey { project: project.clone(), task_name: task_name.clone() };
        if let Some(existing) = graph.get_mut(&key) {
            existing.requested |= requested;
            continue;
        }
        let settings = options.tasks.and_then(|tasks| tasks.get(task_name.as_str()));
        let entries: Vec<String> = match settings {
            Some(settings) => settings.depends_on.clone().unwrap_or_default(),
            None => vec![format!("^{task_name}")],
        };
        let mut dependencies: Vec<TaskKey> = Vec::new();
        let mut seen: HashSet<TaskKey> = HashSet::new();
        for entry in &entries {
            if let Some(dependency_task_name) = entry.strip_prefix('^') {
                for dependency_project in
                    options.project_dependencies.get(&project).into_iter().flatten()
                {
                    let dependency = TaskKey {
                        project: dependency_project.clone(),
                        task_name: dependency_task_name.to_string(),
                    };
                    if seen.insert(dependency.clone()) {
                        dependencies.push(dependency.clone());
                        queue.push_back((dependency.project, dependency.task_name, false));
                    }
                }
            } else {
                let dependency = TaskKey { project: project.clone(), task_name: entry.clone() };
                if seen.insert(dependency.clone()) {
                    dependencies.push(dependency.clone());
                    queue.push_back((dependency.project, dependency.task_name, false));
                }
            }
        }
        let scripts = (options.select_scripts)(&project, &task_name);
        graph.insert(
            key,
            TaskNode {
                project,
                task_name,
                concurrency: settings.and_then(|settings| {
                    settings
                        .concurrency
                        .map(|concurrency| usize::try_from(concurrency).unwrap_or(usize::MAX))
                }),
                scripts,
                requested,
                dependencies,
            },
        );
    }
    graph
}

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
/// limits [`schedule_tasks`] enforces.
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

/// Whether [`schedule_tasks`]'s concurrency limits alone leave at most one
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

/// How the scheduler saw one task end.
pub enum TaskCompletion {
    Passed,
    Failed,
    /// The task was interrupted because another task caused the run to bail.
    Cancelled,
    /// The task's work errored before it could run — an infrastructure
    /// failure, not a script failure. Stops dispatch like a bail.
    Aborted,
}

pub struct ScheduleTasksOptions<'a, Run, Skip>
where
    Run: Fn(&TaskNode) -> TaskCompletion + Sync,
    Skip: Fn(&TaskNode) + Sync,
{
    /// How many tasks may run at once.
    pub concurrency: usize,
    /// When `true`, the first failure stops further dispatch; tasks already
    /// running finish. Tasks never dispatched keep their queued status.
    pub bail: bool,
    /// Runs one task's work. Not called for pass-through tasks (no scripts
    /// to run).
    pub run_task: &'a Run,
    /// A task that runs nothing: a pass-through with no such script, or —
    /// without `--bail` — a task some dependency of which did not pass.
    /// Both are reported as skipped.
    pub on_task_skipped: &'a Skip,
}

/// How [`schedule_graph`] dispatches nodes and responds to failures.
pub struct ScheduleGraphOptions<'a, Run, Skip> {
    /// Maximum number of nodes running at once.
    pub concurrency: usize,
    /// Stop dispatching after the first failure. Takes precedence over
    /// [`Self::continue_on_failure`].
    pub bail: bool,
    /// Let dependents run after a failed dependency when `bail` is false.
    /// Otherwise those dependents are reported as skipped.
    pub continue_on_failure: bool,
    /// Runs one ready node.
    pub run_node: &'a Run,
    /// Reports a node blocked by a failed dependency.
    pub on_node_skipped: &'a Skip,
}

/// How [`schedule_graph_async`] dispatches nodes and responds to failures.
pub struct ScheduleGraphAsyncOptions<'a, Run, Skip> {
    /// Maximum number of nodes running at once.
    pub concurrency: usize,
    /// Stop dispatching after the first failure. Takes precedence over
    /// [`Self::continue_on_failure`].
    pub bail: bool,
    /// Let dependents run after a failed dependency when `bail` is false.
    /// Otherwise those dependents are reported as skipped.
    pub continue_on_failure: bool,
    /// Starts one ready node's future.
    pub run_node: &'a Run,
    /// Reports a node blocked by a failed dependency.
    pub on_node_skipped: &'a Skip,
}

impl<'a, Run, Skip> ScheduleGraphOptions<'a, Run, Skip> {
    pub fn new(
        concurrency: usize,
        bail: bool,
        run_node: &'a Run,
        on_node_skipped: &'a Skip,
    ) -> Self {
        Self { concurrency, bail, continue_on_failure: false, run_node, on_node_skipped }
    }

    #[must_use]
    pub fn continue_on_failure(mut self, continue_on_failure: bool) -> Self {
        self.continue_on_failure = continue_on_failure;
        self
    }
}

impl<'a, Run, Skip> ScheduleGraphAsyncOptions<'a, Run, Skip> {
    pub fn new(
        concurrency: usize,
        bail: bool,
        run_node: &'a Run,
        on_node_skipped: &'a Skip,
    ) -> Self {
        Self { concurrency, bail, continue_on_failure: false, run_node, on_node_skipped }
    }

    #[must_use]
    pub fn continue_on_failure(mut self, continue_on_failure: bool) -> Self {
        self.continue_on_failure = continue_on_failure;
        self
    }
}

struct SchedulerState {
    ready: VecDeque<usize>,
    concurrency_groups: HashMap<String, ConcurrencyGroup>,
    pending_dependencies: Vec<usize>,
    blocked: Vec<bool>,
    settled: Vec<bool>,
    unsettled: usize,
    in_flight: usize,
    stop_dispatch: bool,
}

struct ConcurrencyGroup {
    limit: usize,
    reserved: usize,
    waiting: VecDeque<usize>,
}

struct NodeConcurrencyLimit {
    group: String,
    limit: usize,
}

impl SchedulerState {
    fn make_ready(&mut self, index: usize, limits: &[Option<NodeConcurrencyLimit>]) {
        let Some(limit) = &limits[index] else {
            self.ready.push_back(index);
            return;
        };
        let admitted = {
            let group = self.concurrency_groups.entry(limit.group.clone()).or_insert_with(|| {
                ConcurrencyGroup { limit: limit.limit, reserved: 0, waiting: VecDeque::new() }
            });
            if group.reserved < group.limit {
                group.reserved += 1;
                true
            } else {
                group.waiting.push_back(index);
                false
            }
        };
        if admitted {
            self.ready.push_back(index);
        }
    }

    fn release_concurrency(&mut self, index: usize, limits: &[Option<NodeConcurrencyLimit>]) {
        let Some(limit) = &limits[index] else { return };
        let next = {
            let group = self
                .concurrency_groups
                .get_mut(&limit.group)
                .expect("running task has a concurrency group");
            group.reserved -= 1;
            group.waiting.pop_front().inspect(|_| group.reserved += 1)
        };
        if let Some(next) = next {
            self.ready.push_back(next);
        }
    }
}

/// Dispatch every task whose dependencies have all completed successfully,
/// in dependency order, with at most `concurrency` tasks running at once.
/// Returns once no task can make further progress: all settled, or — under
/// `bail` after a failure — all in-flight work finished.
///
/// The graph must be acyclic ([`sequence_tasks`] proves it); a cycle would
/// deadlock this scheduler.
pub fn schedule_tasks<Run, Skip>(graph: &TaskGraph, options: &ScheduleTasksOptions<'_, Run, Skip>)
where
    Run: Fn(&TaskNode) -> TaskCompletion + Sync,
    Skip: Fn(&TaskNode) + Sync,
{
    let dependencies: IndexMap<TaskKey, Vec<TaskKey>> =
        graph.iter().map(|(key, node)| (key.clone(), node.dependencies.clone())).collect();
    let run_node = |key: TaskKey| {
        let node = &graph[&key];
        if node.scripts.is_empty() {
            (options.on_task_skipped)(node);
            TaskCompletion::Passed
        } else {
            (options.run_task)(node)
        }
    };
    let on_node_skipped = |key: &TaskKey| (options.on_task_skipped)(&graph[key]);
    let concurrency_limit = |key: &TaskKey| {
        let node = &graph[key];
        let concurrency = node.concurrency?;
        (!node.scripts.is_empty()).then(|| NodeConcurrencyLimit {
            group: node.task_name.clone(),
            limit: concurrency.max(1),
        })
    };
    schedule_graph_with_concurrency_limits(
        &dependencies,
        &ScheduleGraphOptions::new(options.concurrency, options.bail, &run_node, &on_node_skipped),
        &concurrency_limit,
    )
    .expect("failed to start a task scheduler worker");
}

/// Dispatch graph nodes as soon as every dependency settles under the
/// configured failure policy. Cyclic edges are broken according to the graph
/// sequencer's deterministic order.
pub fn schedule_graph<Node, Run, Skip>(
    graph: &IndexMap<Node, Vec<Node>>,
    options: &ScheduleGraphOptions<'_, Run, Skip>,
) -> Result<(), std::io::Error>
where
    Node: Clone + Eq + std::hash::Hash + Sync,
    Run: Fn(Node) -> TaskCompletion + Sync,
    Skip: Fn(&Node) + Sync,
{
    schedule_graph_with_concurrency_limits(graph, options, &|_| None)
}

fn schedule_graph_with_concurrency_limits<Node, Run, Skip, Limit>(
    graph: &IndexMap<Node, Vec<Node>>,
    options: &ScheduleGraphOptions<'_, Run, Skip>,
    concurrency_limit: &Limit,
) -> Result<(), std::io::Error>
where
    Node: Clone + Eq + std::hash::Hash + Sync,
    Run: Fn(Node) -> TaskCompletion + Sync,
    Skip: Fn(&Node) + Sync,
    Limit: Fn(&Node) -> Option<NodeConcurrencyLimit>,
{
    if graph.is_empty() {
        return Ok(());
    }
    let included: Vec<Node> = graph.keys().cloned().collect();
    let edges: HashMap<Node, Vec<Node>> =
        graph.iter().map(|(node, dependencies)| (node.clone(), dependencies.clone())).collect();
    let order = graph_sequencer(&edges, &included).order;
    let order_index: HashMap<&Node, usize> =
        order.iter().enumerate().map(|(index, node)| (node, index)).collect();
    let index_of: HashMap<&Node, usize> =
        graph.keys().enumerate().map(|(index, key)| (key, index)).collect();
    let concurrency_limits: Vec<Option<NodeConcurrencyLimit>> =
        graph.keys().map(concurrency_limit).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); graph.len()];
    let mut pending_dependencies: Vec<usize> = vec![0; graph.len()];
    for (index, (node, dependencies)) in graph.iter().enumerate() {
        let dependencies = dependencies.iter().filter(|dependency| {
            order_index
                .get(*dependency)
                .is_some_and(|dependency_index| *dependency_index < order_index[node])
        });
        for dependency in dependencies {
            pending_dependencies[index] += 1;
            dependents[index_of[dependency]].push(index);
        }
    }
    let mut initial_state = SchedulerState {
        ready: VecDeque::new(),
        concurrency_groups: HashMap::new(),
        pending_dependencies,
        blocked: vec![false; graph.len()],
        settled: vec![false; graph.len()],
        unsettled: graph.len(),
        in_flight: 0,
        stop_dispatch: false,
    };
    for index in 0..graph.len() {
        if initial_state.pending_dependencies[index] == 0 {
            initial_state.make_ready(index, &concurrency_limits);
        }
    }
    let state = Mutex::new(initial_state);
    let progress = Condvar::new();

    let complete = |state: &mut SchedulerState, index: usize| {
        state.settled[index] = true;
        state.unsettled -= 1;
        for &dependent in &dependents[index] {
            state.pending_dependencies[dependent] -= 1;
            if state.pending_dependencies[dependent] == 0 && !state.blocked[dependent] {
                state.make_ready(dependent, &concurrency_limits);
            }
        }
    };
    // A failed task's transitive dependents can never become ready (their
    // dependency count never reaches zero), so they are settled here as
    // skipped instead.
    let block = |state: &mut SchedulerState, index: usize| {
        let mut stack = vec![index];
        while let Some(failed) = stack.pop() {
            for &dependent in &dependents[failed] {
                if state.blocked[dependent] {
                    continue;
                }
                state.blocked[dependent] = true;
                state.settled[dependent] = true;
                state.unsettled -= 1;
                (options.on_node_skipped)(
                    graph.get_index(dependent).expect("graph index exists").0,
                );
                stack.push(dependent);
            }
        }
    };

    let workers = options.concurrency.max(1).min(graph.len());
    std::thread::scope(|scope| -> Result<(), std::io::Error> {
        for _ in 0..workers {
            std::thread::Builder::new().spawn_scoped(scope, || {
                let mut guard = state.lock().expect("task scheduler state lock is not poisoned");
                loop {
                    if guard.stop_dispatch {
                        if guard.in_flight == 0 {
                            progress.notify_all();
                            return;
                        }
                        guard = progress
                            .wait(guard)
                            .expect("task scheduler state lock is not poisoned");
                        continue;
                    }
                    let Some(index) = guard.ready.pop_front() else {
                        if guard.unsettled == 0 {
                            progress.notify_all();
                            return;
                        }
                        guard = progress
                            .wait(guard)
                            .expect("task scheduler state lock is not poisoned");
                        continue;
                    };
                    let node = graph.get_index(index).expect("graph index exists").0.clone();
                    guard.in_flight += 1;
                    drop(guard);
                    // A panic in `run_task` must not strand the other
                    // workers: without this guard they would wait forever
                    // on a Condvar nobody signals, and `thread::scope`
                    // would never finish joining them.
                    let panic_guard = AbortOnUnwind { state: &state, progress: &progress };
                    let completion = (options.run_node)(node);
                    drop(panic_guard);
                    guard = state.lock().expect("task scheduler state lock is not poisoned");
                    guard.in_flight -= 1;
                    guard.release_concurrency(index, &concurrency_limits);
                    match completion {
                        TaskCompletion::Passed => complete(&mut guard, index),
                        TaskCompletion::Failed => {
                            if options.bail {
                                guard.settled[index] = true;
                                guard.unsettled -= 1;
                                guard.stop_dispatch = true;
                            } else if options.continue_on_failure {
                                complete(&mut guard, index);
                            } else {
                                guard.settled[index] = true;
                                guard.unsettled -= 1;
                                block(&mut guard, index);
                            }
                        }
                        TaskCompletion::Aborted | TaskCompletion::Cancelled => {
                            guard.settled[index] = true;
                            guard.unsettled -= 1;
                            guard.stop_dispatch = true;
                        }
                    }
                    progress.notify_all();
                }
            })?;
        }
        Ok(())
    })
}

/// Async counterpart of [`schedule_graph`], used by command pipelines whose
/// per-project work is itself asynchronous.
pub async fn schedule_graph_async<Node, Run, Skip, Fut>(
    graph: &IndexMap<Node, Vec<Node>>,
    options: &ScheduleGraphAsyncOptions<'_, Run, Skip>,
) where
    Node: Clone + Eq + std::hash::Hash + Send + Sync,
    Run: Fn(Node) -> Fut + Sync,
    Skip: Fn(&Node) + Sync,
    Fut: std::future::Future<Output = TaskCompletion> + Send,
{
    if graph.is_empty() {
        return;
    }
    let included: Vec<Node> = graph.keys().cloned().collect();
    let edges: HashMap<Node, Vec<Node>> =
        graph.iter().map(|(node, dependencies)| (node.clone(), dependencies.clone())).collect();
    let order = graph_sequencer(&edges, &included).order;
    let order_index: HashMap<&Node, usize> =
        order.iter().enumerate().map(|(index, node)| (node, index)).collect();
    let index_of: HashMap<&Node, usize> =
        graph.keys().enumerate().map(|(index, key)| (key, index)).collect();
    let mut dependents = vec![Vec::new(); graph.len()];
    let mut pending_dependencies = vec![0_usize; graph.len()];
    for (index, (node, dependencies)) in graph.iter().enumerate() {
        for dependency in dependencies.iter().filter(|dependency| {
            order_index
                .get(*dependency)
                .is_some_and(|dependency_index| *dependency_index < order_index[node])
        }) {
            pending_dependencies[index] += 1;
            dependents[index_of[dependency]].push(index);
        }
    }
    let mut ready: VecDeque<usize> = pending_dependencies
        .iter()
        .enumerate()
        .filter(|(_, pending)| **pending == 0)
        .map(|(index, _)| index)
        .collect();
    let mut blocked = vec![false; graph.len()];
    let mut settled = vec![false; graph.len()];
    let mut in_flight = FuturesUnordered::new();
    let mut stop_dispatch = false;
    let mut unsettled = graph.len();
    let concurrency = options.concurrency.max(1);

    while unsettled > 0 && (!stop_dispatch || !in_flight.is_empty()) {
        while !stop_dispatch && in_flight.len() < concurrency {
            let Some(index) = ready.pop_front() else { break };
            let node = graph.get_index(index).expect("graph index exists").0.clone();
            let future = (options.run_node)(node);
            in_flight.push(async move { (index, future.await) });
        }
        let Some((index, completion)) = in_flight.next().await else { break };
        settled[index] = true;
        unsettled -= 1;
        match completion {
            TaskCompletion::Passed => {
                for &dependent in &dependents[index] {
                    pending_dependencies[dependent] -= 1;
                    if pending_dependencies[dependent] == 0 && !blocked[dependent] {
                        ready.push_back(dependent);
                    }
                }
            }
            TaskCompletion::Failed if options.bail => stop_dispatch = true,
            TaskCompletion::Aborted | TaskCompletion::Cancelled => stop_dispatch = true,
            TaskCompletion::Failed if options.continue_on_failure => {
                for &dependent in &dependents[index] {
                    pending_dependencies[dependent] -= 1;
                    if pending_dependencies[dependent] == 0 && !blocked[dependent] {
                        ready.push_back(dependent);
                    }
                }
            }
            TaskCompletion::Failed => {
                let mut stack = vec![index];
                while let Some(failed) = stack.pop() {
                    for &dependent in &dependents[failed] {
                        if blocked[dependent] || settled[dependent] {
                            continue;
                        }
                        blocked[dependent] = true;
                        settled[dependent] = true;
                        unsettled -= 1;
                        (options.on_node_skipped)(
                            graph.get_index(dependent).expect("graph index exists").0,
                        );
                        stack.push(dependent);
                    }
                }
            }
        }
    }
}

/// Settles a panicking worker's in-flight slot and stops dispatch, so the
/// panic propagates out of `thread::scope` instead of deadlocking it.
struct AbortOnUnwind<'a> {
    state: &'a Mutex<SchedulerState>,
    progress: &'a Condvar,
}

impl Drop for AbortOnUnwind<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.in_flight -= 1;
            state.stop_dispatch = true;
        }
        self.progress.notify_all();
    }
}

#[cfg(test)]
mod tests;
