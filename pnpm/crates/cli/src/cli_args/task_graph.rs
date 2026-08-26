//! The task graph of one recursive `run` / `exec` invocation, and the
//! scheduler that dispatches it.
//!
//! A task is a `(project, task name)` pair. A task becomes runnable when
//! every task it depends on has completed successfully; runnable tasks are
//! dispatched under the `workspaceConcurrency` limit, with no barrier
//! between dependency-independent tasks. Mirrors `taskGraph.ts` /
//! `taskScheduler.ts` in pnpm's `@pnpm/exec.commands`.

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::Diagnostic;
use pnpm_config::TaskSettings;
use pnpm_package_manager::graph_sequencer;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
};

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
#[diagnostic(code(ERR_PNPM_TASK_CYCLE))]
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
        let entries: Vec<String> =
            match options.tasks.and_then(|tasks| tasks.get(task_name.as_str())) {
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
        graph.insert(key, TaskNode { project, task_name, scripts, requested, dependencies });
    }
    graph
}

/// Topologically sequence the task graph into ready-together groups —
/// dependencies always in an earlier group — erroring when the tasks form
/// a cycle. Detection is scoped to this graph: a cycle among tasks the
/// filter did not select cannot fail the run.
pub fn sequence_tasks(
    graph: &TaskGraph,
    workspace_dir: &Path,
) -> Result<Vec<Vec<TaskKey>>, TaskCycle> {
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
                    .map(|key| format_task(key, workspace_dir))
                    .collect::<Vec<_>>()
                    .join(" → ")
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(TaskCycle { cycles });
    }
    Ok(result.chunks)
}

/// `<workspace-relative dir>#<task name>`, with forward slashes on every
/// platform — the rendering of a task in cycle errors and dry-run output.
pub fn format_task(key: &TaskKey, workspace_dir: &Path) -> String {
    format!("{}#{}", relative_project_dir(&key.project, workspace_dir), key.task_name)
}

fn relative_project_dir(project: &Path, workspace_dir: &Path) -> String {
    let relative = pathdiff::diff_paths(project, workspace_dir).unwrap_or_default();
    if relative.as_os_str().is_empty() {
        return ".".to_string();
    }
    relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// The same graph with every edge turned around: dependents run before
/// dependencies.
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
pub fn resume_task_graph_from(
    graph: TaskGraph,
    anchor_project: &Path,
    task_name: &str,
) -> TaskGraph {
    let anchor =
        TaskKey { project: anchor_project.to_path_buf(), task_name: task_name.to_string() };
    let Some(anchor_node) = graph.get(&anchor) else {
        // The anchor exists but its task is not in this graph: there is
        // nothing to skip.
        return graph;
    };
    let mut dropped: HashSet<TaskKey> = HashSet::new();
    let mut stack: Vec<TaskKey> = anchor_node.dependencies.clone();
    while let Some(key) = stack.pop() {
        if !dropped.insert(key.clone()) {
            continue;
        }
        stack.extend(graph[&key].dependencies.iter().cloned());
    }
    graph
        .into_iter()
        .filter(|(key, _)| !dropped.contains(key))
        .map(|(key, mut node)| {
            node.dependencies.retain(|dependency| !dropped.contains(dependency));
            (key, node)
        })
        .collect()
}

/// Whether at most one script can ever be in flight, which is when output
/// may stay inherited rather than piped: no task runs several scripts, and
/// every script-running task lies on one dependency chain, so the graph
/// forces them to run one after another.
///
/// `sequenced_tasks` is [`sequence_tasks`]'s result — the proof the graph
/// is acyclic, and the evaluation order for the longest-chain scan.
pub fn is_serial_task_graph(graph: &TaskGraph, sequenced_tasks: &[Vec<TaskKey>]) -> bool {
    let mut script_task_count = 0_usize;
    for node in graph.values() {
        if node.scripts.len() > 1 {
            return false;
        }
        script_task_count += node.scripts.len();
    }
    if script_task_count <= 1 {
        return true;
    }
    let mut chain_length: HashMap<&TaskKey, usize> = HashMap::new();
    let mut longest_chain = 0_usize;
    for group in sequenced_tasks {
        for key in group {
            let node = &graph[key];
            let via_dependencies = node
                .dependencies
                .iter()
                .map(|dependency| chain_length.get(dependency).copied().unwrap_or(0))
                .max()
                .unwrap_or(0);
            let length = via_dependencies + node.scripts.len();
            chain_length
                .insert(graph.get_key_value(key).expect("sequenced key is in graph").0, length);
            longest_chain = longest_chain.max(length);
        }
    }
    longest_chain == script_task_count
}

/// The task's key in the recursive summary. The task of the script the
/// invocation named keeps the project directory alone — the format
/// existing consumers of `pnpm-exec-summary.json` read — and only tasks
/// `dependsOn` pulled in qualify it with the task name.
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
pub fn render_task_graph_dry_run(
    graph: &TaskGraph,
    sequenced_tasks: &[Vec<TaskKey>],
    workspace_dir: &Path,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    for group in sequenced_tasks {
        let mut rendered: Vec<(String, bool)> = group
            .iter()
            .map(|key| (format_task(key, workspace_dir), graph[key].scripts.is_empty()))
            .collect();
        rendered.sort();
        for (task, missing_script) in rendered {
            lines.push(if missing_script {
                format!("{task} (skipped: no such script)")
            } else {
                task
            });
        }
    }
    lines.join("\n")
}

/// How the scheduler saw one task end.
pub enum TaskCompletion {
    Passed,
    Failed,
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

struct SchedulerState {
    ready: VecDeque<usize>,
    pending_dependencies: Vec<usize>,
    blocked: Vec<bool>,
    settled: Vec<bool>,
    unsettled: usize,
    in_flight: usize,
    stop_dispatch: bool,
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
    if graph.is_empty() {
        return;
    }
    let index_of: HashMap<&TaskKey, usize> =
        graph.keys().enumerate().map(|(index, key)| (key, index)).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); graph.len()];
    let mut pending_dependencies: Vec<usize> = vec![0; graph.len()];
    for (index, node) in graph.values().enumerate() {
        pending_dependencies[index] = node.dependencies.len();
        for dependency in &node.dependencies {
            dependents[index_of[dependency]].push(index);
        }
    }
    let state = Mutex::new(SchedulerState {
        ready: pending_dependencies
            .iter()
            .enumerate()
            .filter(|(_, pending)| **pending == 0)
            .map(|(index, _)| index)
            .collect(),
        pending_dependencies,
        blocked: vec![false; graph.len()],
        settled: vec![false; graph.len()],
        unsettled: graph.len(),
        in_flight: 0,
        stop_dispatch: false,
    });
    let progress = Condvar::new();

    let complete = |state: &mut SchedulerState, index: usize| {
        state.settled[index] = true;
        state.unsettled -= 1;
        for &dependent in &dependents[index] {
            state.pending_dependencies[dependent] -= 1;
            if state.pending_dependencies[dependent] == 0 && !state.blocked[dependent] {
                state.ready.push_back(dependent);
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
                (options.on_task_skipped)(&graph[dependent]);
                stack.push(dependent);
            }
        }
    };

    let workers = options.concurrency.max(1).min(graph.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
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
                    let node = &graph[index];
                    if node.scripts.is_empty() {
                        (options.on_task_skipped)(node);
                        complete(&mut guard, index);
                        progress.notify_all();
                        continue;
                    }
                    guard.in_flight += 1;
                    drop(guard);
                    let completion = (options.run_task)(node);
                    guard = state.lock().expect("task scheduler state lock is not poisoned");
                    guard.in_flight -= 1;
                    match completion {
                        TaskCompletion::Passed => complete(&mut guard, index),
                        TaskCompletion::Failed => {
                            guard.settled[index] = true;
                            guard.unsettled -= 1;
                            if options.bail {
                                guard.stop_dispatch = true;
                            } else {
                                block(&mut guard, index);
                            }
                        }
                        TaskCompletion::Aborted => {
                            guard.settled[index] = true;
                            guard.unsettled -= 1;
                            guard.stop_dispatch = true;
                        }
                    }
                    progress.notify_all();
                }
            });
        }
    });
}

#[cfg(test)]
mod tests;
