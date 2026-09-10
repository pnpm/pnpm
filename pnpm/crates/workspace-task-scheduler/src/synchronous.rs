use super::{
    AbortOnUnwind, Condvar, HashMap, IndexMap, Mutex, MutexGuard, NodeConcurrencyLimit, NodeEdges,
    ScheduleGraphOptions, ScheduleTasksOptions, SchedulerState, TaskCompletion, TaskGraph, TaskKey,
    TaskNode, TaskSettings, VecDeque, graph_sequencer,
};

/// Dispatch every task whose dependencies have all completed successfully,
/// in dependency order, with at most `concurrency` tasks running at once.
/// Returns once no task can make further progress: all settled, or — under
/// `bail` after a failure — all in-flight work finished.
///
/// The graph must be acyclic ([`sequence_tasks`](crate::graph::sequence_tasks) proves it); a cycle would
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
    let NodeEdges { dependents, pending_dependencies } = node_edges(graph);
    let scheduling = Scheduling {
        graph,
        options,
        dependents,
        concurrency_limits: graph.keys().map(concurrency_limit).collect(),
    };
    let state =
        Mutex::new(initial_scheduler_state(pending_dependencies, &scheduling.concurrency_limits));
    let progress = Condvar::new();

    let workers = options.concurrency.max(1).min(graph.len());
    std::thread::scope(|scope| -> Result<(), std::io::Error> {
        for _ in 0..workers {
            std::thread::Builder::new()
                .spawn_scoped(scope, || scheduling.work(&state, &progress))?;
        }
        Ok(())
    })
}

pub(super) fn node_edges<Node: Clone + Eq + std::hash::Hash>(
    graph: &IndexMap<Node, Vec<Node>>,
) -> NodeEdges {
    let order = sequenced_order(graph);
    let order_index: HashMap<&Node, usize> =
        order.iter().enumerate().map(|(index, node)| (node, index)).collect();
    let index_of: HashMap<&Node, usize> =
        graph.keys().enumerate().map(|(index, key)| (key, index)).collect();

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
    NodeEdges { dependents, pending_dependencies }
}

pub(super) fn task_concurrency(settings: &TaskSettings) -> Option<usize> {
    settings.concurrency.map(|concurrency| usize::try_from(concurrency).unwrap_or(usize::MAX))
}

fn sequenced_order<Node: Clone + Eq + std::hash::Hash>(
    graph: &IndexMap<Node, Vec<Node>>,
) -> Vec<Node> {
    let included: Vec<Node> = graph.keys().cloned().collect();
    let edges: HashMap<Node, Vec<Node>> =
        graph.iter().map(|(node, dependencies)| (node.clone(), dependencies.clone())).collect();
    graph_sequencer(&edges, &included).order
}

fn initial_scheduler_state(
    pending_dependencies: Vec<usize>,
    limits: &[Option<NodeConcurrencyLimit>],
) -> SchedulerState {
    let node_count = pending_dependencies.len();
    let mut state = SchedulerState {
        ready: VecDeque::new(),
        concurrency_groups: HashMap::new(),
        pending_dependencies,
        blocked: vec![false; node_count],
        settled: vec![false; node_count],
        unsettled: node_count,
        in_flight: 0,
        stop_dispatch: false,
    };
    for index in 0..node_count {
        if state.pending_dependencies[index] == 0 {
            state.make_ready(index, limits);
        }
    }
    state
}

/// Everything a scheduler worker needs beyond the shared mutable state.
struct Scheduling<'a, Node, Run, Skip> {
    graph: &'a IndexMap<Node, Vec<Node>>,
    options: &'a ScheduleGraphOptions<'a, Run, Skip>,
    dependents: Vec<Vec<usize>>,
    concurrency_limits: Vec<Option<NodeConcurrencyLimit>>,
}

impl<Node, Run, Skip> Scheduling<'_, Node, Run, Skip>
where
    Node: Clone + Eq + std::hash::Hash + Sync,
    Run: Fn(Node) -> TaskCompletion + Sync,
    Skip: Fn(&Node) + Sync,
{
    /// Run dispatched tasks until nothing is left for this worker to do.
    fn work(&self, state: &Mutex<SchedulerState>, progress: &Condvar) {
        let mut guard = state.lock().expect("task scheduler state lock is not poisoned");
        loop {
            let (returned, ready) = take_ready(guard, progress);
            guard = returned;
            let Some(index) = ready else { return };

            let node = self.graph.get_index(index).expect("graph index exists").0.clone();
            guard.in_flight += 1;
            drop(guard);
            // A panic in `run_node` must not strand the other
            // workers: without this guard they would wait forever
            // on a Condvar nobody signals, and `thread::scope`
            // would never finish joining them.
            let panic_guard = AbortOnUnwind { state, progress };
            let completion = (self.options.run_node)(node);
            drop(panic_guard);

            guard = state.lock().expect("task scheduler state lock is not poisoned");
            guard.in_flight -= 1;
            guard.release_concurrency(index, &self.concurrency_limits);
            self.settle(&mut guard, index, completion);
            progress.notify_all();
        }
    }

    fn settle(&self, state: &mut SchedulerState, index: usize, completion: TaskCompletion) {
        match completion {
            TaskCompletion::Passed => {
                state.complete(index, &self.dependents, &self.concurrency_limits);
            }
            TaskCompletion::Failed if self.options.bail => state.stop(index),
            TaskCompletion::Failed if self.options.continue_on_failure => {
                state.complete(index, &self.dependents, &self.concurrency_limits);
            }
            TaskCompletion::Failed => {
                state.settled[index] = true;
                state.unsettled -= 1;
                state.block(index, &self.dependents, |dependent| {
                    (self.options.on_node_skipped)(self.node_at(dependent));
                });
            }
            TaskCompletion::Aborted | TaskCompletion::Cancelled => state.stop(index),
        }
    }

    fn node_at(&self, index: usize) -> &Node {
        self.graph.get_index(index).expect("graph index exists").0
    }
}

/// Block until a task is ready to dispatch. Returns the guard, and the task
/// index unless the calling worker has nothing left to do.
fn take_ready<'state>(
    mut guard: MutexGuard<'state, SchedulerState>,
    progress: &Condvar,
) -> (MutexGuard<'state, SchedulerState>, Option<usize>) {
    loop {
        if guard.stop_dispatch {
            if guard.in_flight == 0 {
                progress.notify_all();
                return (guard, None);
            }
        } else if let Some(index) = guard.ready.pop_front() {
            return (guard, Some(index));
        } else if guard.unsettled == 0 {
            progress.notify_all();
            return (guard, None);
        }
        guard = progress.wait(guard).expect("task scheduler state lock is not poisoned");
    }
}
