import { graphSequencer } from '@pnpm/deps.graph-sequencer'

import type { TaskGraph, TaskKey, TaskNode } from './taskGraph.js'

export type DependencyGraph<Node> = Map<Node, Node[]>

export type TaskCompletion =
  | 'passed'
  | 'failed'
  /**
   * The task's work errored before it could run — an infrastructure
   * failure, not a script failure. Stops dispatch like a bail; the caller
   * holds the error and rethrows it after the scheduler settles.
   */
  | 'aborted'

export interface ScheduleTasksOptions {
  /** When `true`, the first failure stops the run: nothing further is dispatched and the scheduler settles at once. */
  bail: boolean
  /**
   * Runs one task's work and resolves with how it ended. Never rejects:
   * the caller records its own failure details. Not called for
   * pass-through tasks (no scripts to run).
   */
  runTask: (node: TaskNode, key: TaskKey) => Promise<TaskCompletion>
  /**
   * A task that runs nothing: a pass-through with no such script, or —
   * without `--bail` — a task some dependency of which did not pass. Both are
   * reported as skipped.
   */
  onTaskSkipped: (node: TaskNode, key: TaskKey) => void
}

export interface ScheduleGraphOptions<Node> {
  /** See {@link ScheduleTasksOptions.bail}. */
  bail: boolean
  /** Maximum number of graph nodes whose work may be in flight. */
  concurrency?: number
  /** Let dependents run after a failed node. Used by legacy `--no-bail` command loops. */
  continueOnFailure?: boolean
  /** Wait for already-dispatched nodes after dispatch stops. Defaults to `true`. */
  finishInFlight?: boolean
  runNode: (node: Node) => Promise<TaskCompletion>
  onNodeSkipped: (node: Node) => void
}

/**
 * Dispatches every task whose dependencies have all completed successfully,
 * in dependency order and nothing else, with concurrency among ready tasks
 * limited by the scheduler. Resolves once all tasks settled, or as
 * soon as a bailed failure or an abort stops the run: in-flight work is then
 * abandoned to the caller, whose exit path terminates the running commands.
 * Tasks never dispatched are left untouched, so their caller-side status
 * stays whatever "queued" is.
 *
 * The graph must be acyclic ({@link sequenceTasks} proves it); a cycle would
 * hang this scheduler.
 */
export async function scheduleTasks (graph: TaskGraph, opts: ScheduleTasksOptions): Promise<void> {
  const dependencies: DependencyGraph<TaskKey> = new Map()
  for (const [key, node] of graph) {
    dependencies.set(key, node.dependencies)
  }
  await scheduleGraphWithConcurrencyLimits(dependencies, {
    bail: opts.bail,
    finishInFlight: false,
    runNode: async (key) => {
      const node = graph.get(key)!
      if (node.scripts.length === 0) {
        opts.onTaskSkipped(node, key)
        return 'passed'
      }
      return opts.runTask(node, key)
    },
    onNodeSkipped: (key) => opts.onTaskSkipped(graph.get(key)!, key),
  }, (key) => {
    const node = graph.get(key)!
    return node.concurrency == null || node.scripts.length === 0
      ? undefined
      : { group: node.taskName, limit: normalizeConcurrency(node.concurrency) }
  })
}

/**
 * Dispatches graph nodes as soon as all of their dependencies settle under the
 * configured failure policy. Independent branches do not wait for a shared
 * topological-group barrier. Backward edges in a cycle are dropped according
 * to the graph sequencer's deterministic order.
 */
export async function scheduleGraph<Node> (
  graph: DependencyGraph<Node>,
  opts: ScheduleGraphOptions<Node>
): Promise<void> {
  await scheduleGraphWithConcurrencyLimits(graph, opts)
}

interface ConcurrencyLimit {
  group: string
  limit: number
}

interface ConcurrencyGroup<Node> {
  limit: number
  reserved: number
  waiting: Node[]
  waitingHead: number
}

interface Scheduler<Node> {
  opts: ScheduleGraphOptions<Node>
  concurrency: number
  concurrencyLimit?: (node: Node) => ConcurrencyLimit | undefined
  pendingDependencyCount: Map<Node, number>
  dependents: Map<Node, Node[]>
  /**
   * Drained by index: shift() moves every remaining element, which is
   * quadratic over a workspace-sized queue.
   */
  ready: Node[]
  readyHead: number
  active: number
  pumping: boolean
  blocked: Set<Node>
  stopDispatch: boolean
  unsettled: number
  nodeConcurrencyGroups: Map<Node, string>
  concurrencyGroups: Map<string, ConcurrencyGroup<Node>>
  /**
   * A rejection violates runTask's contract; held here so the run still
   * fails with it rather than silently resolving. First error wins: a
   * rejection landing only after something else already stopped the run is
   * abandoned along with the rest of the in-flight work, exactly as a
   * second script failure after a bail is.
   */
  contractViolation?: { error: unknown }
  resolve: () => void
}

async function scheduleGraphWithConcurrencyLimits<Node> (
  graph: DependencyGraph<Node>,
  opts: ScheduleGraphOptions<Node>,
  concurrencyLimit?: (node: Node) => ConcurrencyLimit | undefined
): Promise<void> {
  const concurrency = normalizeConcurrency(opts.concurrency)
  const { pendingDependencyCount, dependents } = indexOrderedDependencies(graph)
  const scheduler: Scheduler<Node> = {
    opts,
    concurrency,
    concurrencyLimit,
    pendingDependencyCount,
    dependents,
    ready: [],
    readyHead: 0,
    active: 0,
    pumping: false,
    blocked: new Set(),
    stopDispatch: false,
    unsettled: graph.size,
    nodeConcurrencyGroups: new Map(),
    concurrencyGroups: new Map(),
    resolve: () => {},
  }
  for (const [node, count] of pendingDependencyCount) {
    if (count === 0) makeReady(scheduler, node)
  }
  await new Promise<void>((resolve) => {
    scheduler.resolve = resolve
    pump(scheduler)
  })
  if (scheduler.contractViolation != null) {
    throw scheduler.contractViolation.error
  }
}

/**
 * Counts each node's dependencies and indexes its dependents, keeping only
 * the edges that point backward in the graph sequencer's order.
 */
function indexOrderedDependencies<Node> (graph: DependencyGraph<Node>): {
  pendingDependencyCount: Map<Node, number>
  dependents: Map<Node, Node[]>
} {
  const pendingDependencyCount = new Map<Node, number>()
  const dependents = new Map<Node, Node[]>()
  const order = graphSequencer(graph).order
  const orderIndex = new Map(order.map((node, index) => [node, index]))
  for (const [node, dependencies] of graph) {
    const orderedDependencies = dependencies.filter(
      (dependency) => orderIndex.get(dependency)! < orderIndex.get(node)!
    )
    pendingDependencyCount.set(node, orderedDependencies.length)
    for (const dependency of orderedDependencies) {
      addDependent(dependents, dependency, node)
    }
  }
  return { pendingDependencyCount, dependents }
}

function addDependent<Node> (dependents: Map<Node, Node[]>, dependency: Node, dependent: Node): void {
  let list = dependents.get(dependency)
  if (list == null) {
    dependents.set(dependency, list = [])
  }
  list.push(dependent)
}

function makeReady<Node> (scheduler: Scheduler<Node>, node: Node): void {
  const concurrency = scheduler.concurrencyLimit?.(node)
  if (concurrency == null) {
    scheduler.ready.push(node)
    return
  }
  scheduler.nodeConcurrencyGroups.set(node, concurrency.group)
  const group = getOrCreateConcurrencyGroup(scheduler.concurrencyGroups, concurrency)
  if (group.reserved < group.limit) {
    group.reserved++
    scheduler.ready.push(node)
  } else {
    group.waiting.push(node)
  }
}

function getOrCreateConcurrencyGroup<Node> (
  concurrencyGroups: Map<string, ConcurrencyGroup<Node>>,
  concurrency: ConcurrencyLimit
): ConcurrencyGroup<Node> {
  let group = concurrencyGroups.get(concurrency.group)
  if (group == null) {
    concurrencyGroups.set(concurrency.group, group = {
      limit: concurrency.limit,
      reserved: 0,
      waiting: [],
      waitingHead: 0,
    })
  }
  return group
}

function releaseConcurrency<Node> (scheduler: Scheduler<Node>, node: Node): void {
  const groupName = scheduler.nodeConcurrencyGroups.get(node)
  if (groupName == null) return
  const group = scheduler.concurrencyGroups.get(groupName)!
  group.reserved--
  if (group.waitingHead < group.waiting.length) {
    group.reserved++
    scheduler.ready.push(group.waiting[group.waitingHead++])
  }
}

/**
 * An explicit queue rather than recursion: a workspace-long chain of
 * pass-through tasks completes synchronously, and call depth must not grow
 * with chain length.
 */
function pump<Node> (scheduler: Scheduler<Node>): void {
  if (scheduler.pumping) return
  scheduler.pumping = true
  while (canDispatch(scheduler)) {
    dispatch(scheduler, scheduler.ready[scheduler.readyHead++])
  }
  scheduler.pumping = false
  settleIfDone(scheduler)
}

function canDispatch<Node> (scheduler: Scheduler<Node>): boolean {
  return !scheduler.stopDispatch &&
    scheduler.active < scheduler.concurrency &&
    scheduler.readyHead < scheduler.ready.length
}

function dispatch<Node> (scheduler: Scheduler<Node>, node: Node): void {
  scheduler.active++
  scheduler.opts.runNode(node).then((completion) => {
    finishNode(scheduler, node, completion)
  }, (error: unknown) => {
    // runTask's contract is to never reject; treated as an abort, and
    // the error resurfaces once the scheduler settles.
    scheduler.contractViolation ??= { error }
    finishNode(scheduler, node, 'aborted')
  })
}

function finishNode<Node> (scheduler: Scheduler<Node>, node: Node, completion: TaskCompletion): void {
  scheduler.active--
  releaseConcurrency(scheduler, node)
  settle(scheduler, node, completion)
  pump(scheduler)
}

function settleIfDone<Node> (scheduler: Scheduler<Node>): void {
  // Task runs may opt out because a watch-style script never finishes.
  // Command pipelines wait for work that was already dispatched.
  const inFlightSettled = scheduler.opts.finishInFlight === false || scheduler.active === 0
  if (scheduler.unsettled === 0 || (scheduler.stopDispatch && inFlightSettled)) {
    scheduler.resolve()
  }
}

function settle<Node> (scheduler: Scheduler<Node>, node: Node, completion: TaskCompletion): void {
  switch (completion) {
    case 'passed':
      complete(scheduler, node)
      break
    case 'failed':
      if (scheduler.opts.bail) {
        scheduler.unsettled--
        scheduler.stopDispatch = true
      } else if (scheduler.opts.continueOnFailure === true) {
        complete(scheduler, node)
      } else {
        scheduler.unsettled--
        block(scheduler, node)
      }
      break
    case 'aborted':
      scheduler.unsettled--
      scheduler.stopDispatch = true
      break
  }
}

function complete<Node> (scheduler: Scheduler<Node>, node: Node): void {
  scheduler.unsettled--
  for (const dependent of scheduler.dependents.get(node) ?? []) {
    const remaining = scheduler.pendingDependencyCount.get(dependent)! - 1
    scheduler.pendingDependencyCount.set(dependent, remaining)
    if (remaining === 0 && !scheduler.blocked.has(dependent)) {
      makeReady(scheduler, dependent)
    }
  }
}

/**
 * A failed task's transitive dependents can never become ready (their
 * dependency count never reaches zero), so they are settled here as skipped
 * instead.
 */
function block<Node> (scheduler: Scheduler<Node>, node: Node): void {
  const stack = [node]
  while (stack.length > 0) {
    for (const dependent of scheduler.dependents.get(stack.pop()!) ?? []) {
      if (scheduler.blocked.has(dependent)) continue
      scheduler.blocked.add(dependent)
      scheduler.unsettled--
      scheduler.opts.onNodeSkipped(dependent)
      stack.push(dependent)
    }
  }
}

function normalizeConcurrency (concurrency: number | undefined): number {
  if (concurrency === Infinity || concurrency == null) return Infinity
  return Number.isInteger(concurrency) && concurrency > 0 ? concurrency : 1
}
