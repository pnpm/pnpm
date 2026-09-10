use super::{
    Condvar, FuturesUnordered, IndexMap, Mutex, NodeEdges, ScheduleGraphAsyncOptions,
    SchedulerState, StreamExt, TaskCompletion, VecDeque, node_edges,
};

/// Async counterpart of [`schedule_graph`](crate::synchronous::schedule_graph), used by command pipelines whose
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
    let NodeEdges { dependents, pending_dependencies } = node_edges(graph);
    let mut state = AsyncState {
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
        stop_dispatch: false,
    };
    let policy = FailurePolicy { bail: options.bail, continue_on: options.continue_on_failure };
    let mut in_flight = FuturesUnordered::new();
    let concurrency = options.concurrency.max(1);

    while state.unsettled > 0 {
        while let Some(index) = state.next_dispatch(in_flight.len(), concurrency) {
            let node = graph.get_index(index).expect("graph index exists").0.clone();
            let future = (options.run_node)(node);
            in_flight.push(async move { (index, future.await) });
        }
        // Nothing in flight and nothing dispatchable: no task can make
        // further progress.
        let Some((index, completion)) = in_flight.next().await else { break };
        state.settle(index, completion, &dependents, policy, |dependent| {
            (options.on_node_skipped)(graph.get_index(dependent).expect("graph index exists").0);
        });
    }
}

/// How a failed task settles: `bail` stops dispatch outright, `continue_on`
/// runs its dependents anyway, and neither skips them.
#[derive(Clone, Copy)]
struct FailurePolicy {
    bail: bool,
    continue_on: bool,
}

/// The mutable half of [`schedule_graph_async`]. Concurrency is capped
/// globally rather than per group, so there is no group bookkeeping.
struct AsyncState {
    ready: VecDeque<usize>,
    pending_dependencies: Vec<usize>,
    blocked: Vec<bool>,
    settled: Vec<bool>,
    unsettled: usize,
    stop_dispatch: bool,
}

impl AsyncState {
    /// The next task to start, or `None` when dispatch has stopped, the
    /// concurrency limit is reached, or nothing is ready.
    fn next_dispatch(&mut self, in_flight: usize, concurrency: usize) -> Option<usize> {
        if self.stop_dispatch || in_flight >= concurrency {
            return None;
        }
        self.ready.pop_front()
    }

    fn settle(
        &mut self,
        index: usize,
        completion: TaskCompletion,
        dependents: &[Vec<usize>],
        policy: FailurePolicy,
        on_skipped: impl Fn(usize),
    ) {
        self.settled[index] = true;
        self.unsettled -= 1;
        match completion {
            TaskCompletion::Passed => self.release_dependents(index, dependents),
            TaskCompletion::Failed if policy.bail => self.stop_dispatch = true,
            TaskCompletion::Aborted | TaskCompletion::Cancelled => self.stop_dispatch = true,
            TaskCompletion::Failed if policy.continue_on => {
                self.release_dependents(index, dependents);
            }
            TaskCompletion::Failed => self.block(index, dependents, on_skipped),
        }
    }

    fn release_dependents(&mut self, index: usize, dependents: &[Vec<usize>]) {
        for &dependent in &dependents[index] {
            self.pending_dependencies[dependent] -= 1;
            if self.pending_dependencies[dependent] == 0 && !self.blocked[dependent] {
                self.ready.push_back(dependent);
            }
        }
    }

    /// A failed task's transitive dependents can never become ready (their
    /// dependency count never reaches zero), so they are settled here as
    /// skipped instead.
    fn block(&mut self, index: usize, dependents: &[Vec<usize>], on_skipped: impl Fn(usize)) {
        let mut stack = vec![index];
        while let Some(failed) = stack.pop() {
            for &dependent in &dependents[failed] {
                if self.blocked[dependent] || self.settled[dependent] {
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
}

/// Settles a panicking worker's in-flight slot and stops dispatch, so the
/// panic propagates out of `thread::scope` instead of deadlocking it.
pub(super) struct AbortOnUnwind<'a> {
    pub(super) state: &'a Mutex<SchedulerState>,
    pub(super) progress: &'a Condvar,
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
