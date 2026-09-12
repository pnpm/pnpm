//! The task graph of one recursive `run` / `exec` invocation, and the
//! scheduler that dispatches it.
//!
//! A task is a `(project, task name)` pair. A task becomes runnable when
//! every task it depends on has completed successfully; runnable tasks are
//! dispatched under the `workspaceConcurrency` limit, with no barrier
//! between dependency-independent tasks. Mirrors `taskGraph.ts` /
//! `taskScheduler.ts` in pnpm's `@pnpm/workspace.task-scheduler`.

pub use asynchronous::schedule_graph_async;
pub use build::{build_pipeline_task_graph, build_task_graph};
pub use graph::{
    DryRunDocument, DryRunTask, DryRunTaskDependency, SequenceTasksOptions, format_task,
    is_serial_task_graph, render_task_graph_dry_run, resume_task_graph_from, reverse_task_graph,
    sequence_tasks, task_graph_to_json, task_summary_key,
};
pub use graph_sequencer::{GraphSequencerResult, PathNode, graph_sequencer};
pub use synchronous::{schedule_graph, schedule_tasks};

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
    sync::{Condvar, Mutex, MutexGuard},
};

mod graph_sequencer;

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
    /// The projects the graph can contain and their dependency edges.
    /// Every dependency and requested project must be present in this map.
    /// Callers that apply `--filter` narrow both the map and the requested
    /// projects to preserve the filter's execution boundary.
    pub project_dependencies: &'a IndexMap<PathBuf, Vec<PathBuf>>,
    pub select_scripts: SelectScripts,
    /// The script the invocation requests in each requested project.
    pub task_name: &'a str,
    /// The projects whose tasks the invocation requests, in dispatch
    /// tie-break order. `None` requests every project in the map's order.
    /// Other projects participate only through `dependsOn` edges.
    pub requested_projects: Option<&'a [PathBuf]>,
    pub tasks: Option<&'a IndexMap<String, TaskSettings>>,
}

pub struct BuildPipelineTaskGraphOptions<'a, SelectScripts>
where
    SelectScripts: Fn(&Path, &str) -> Vec<String>,
{
    /// The dependency edges among the included projects, already resolved
    /// through the full workspace graph. Tasks are created only for these
    /// projects, so the map must be dependency-closed: a task's `^` edges
    /// resolve identically whatever narrowed the run, which is what keeps
    /// its cache key selection-independent.
    pub project_dependencies: &'a IndexMap<PathBuf, Vec<PathBuf>>,
    pub select_scripts: SelectScripts,
    /// The tasks the pipeline requests; every requested project gets a
    /// task per name. A pipeline is a set, so the names carry no
    /// ordering — any ordering among them is `dependsOn`'s job.
    pub task_names: &'a [&'a str],
    /// The projects whose tasks the invocation requests — the affected
    /// set. `None` requests every project of `project_dependencies`. The
    /// rest of the map participates only through `dependsOn` edges, the
    /// way an upstream build is pulled in without its lint or tests.
    pub requested_projects: Option<&'a [PathBuf]>,
    pub tasks: Option<&'a IndexMap<String, TaskSettings>>,
}

/// How the scheduler saw one task end.
#[derive(Clone, Copy)]
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

/// The forward edges of the acyclic graph the sequencer settled on, by
/// position: for each node the nodes waiting on it, and how many
/// dependencies it is itself still waiting for. Edges the sequencer put out
/// of order are cycle-breaking back edges and are dropped, so the counts
/// always drain.
struct NodeEdges {
    dependents: Vec<Vec<usize>>,
    pending_dependencies: Vec<usize>,
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

    /// Settle `index` as passed and make ready every dependent it was the
    /// last dependency of.
    fn complete(
        &mut self,
        index: usize,
        dependents: &[Vec<usize>],
        limits: &[Option<NodeConcurrencyLimit>],
    ) {
        self.settled[index] = true;
        self.unsettled -= 1;
        for &dependent in &dependents[index] {
            self.pending_dependencies[dependent] -= 1;
            if self.pending_dependencies[dependent] == 0 && !self.blocked[dependent] {
                self.make_ready(dependent, limits);
            }
        }
    }

    /// Settle `index` and stop dispatching anything further.
    fn stop(&mut self, index: usize) {
        self.settled[index] = true;
        self.unsettled -= 1;
        self.stop_dispatch = true;
    }

    /// A failed task's transitive dependents can never become ready (their
    /// dependency count never reaches zero), so they are settled here as
    /// skipped instead.
    fn block(&mut self, index: usize, dependents: &[Vec<usize>], on_skipped: impl Fn(usize)) {
        let mut stack = vec![index];
        while let Some(failed) = stack.pop() {
            for &dependent in &dependents[failed] {
                if self.blocked[dependent] {
                    continue;
                }
                self.blocked[dependent] = true;
                self.settled[dependent] = true;
                self.unsettled -= 1;
                on_skipped(dependent);
                stack.push(dependent);
            }
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

#[cfg(test)]
mod tests;

mod build;

mod graph;

mod synchronous;
use synchronous::{node_edges, task_concurrency};

mod asynchronous;
use asynchronous::AbortOnUnwind;
