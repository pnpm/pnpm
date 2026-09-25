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
  /** When `true`, the first failure stops the run: nothing further is dispatched and the scheduler settles at once. */
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

async function scheduleGraphWithConcurrencyLimits<Node> (
  graph: DependencyGraph<Node>,
  opts: ScheduleGraphOptions<Node>,
  concurrencyLimit?: (node: Node) => ConcurrencyLimit | undefined
): Promise<void> {
  // A rejection violates runTask's contract; held here so the run still
  // fails with it rather than silently resolving. First error wins: a
  // rejection landing only after something else already stopped the run is
  // abandoned along with the rest of the in-flight work, exactly as a
  // second script failure after a bail is.
  let contractViolation: unknown
  let rejected = false
  const concurrency = normalizeConcurrency(opts.concurrency)
  const pendingDependencyCount = new Map<Node, number>()
  const dependents = new Map<Node, Node[]>()
  const ready: Node[] = []
  const nodeConcurrencyGroups = new Map<Node, string>()
  const concurrencyGroups = new Map<string, ConcurrencyGroup<Node>>()
  const order = graphSequencer(graph).order
  const orderIndex = new Map(order.map((node, index) => [node, index]))
  for (const [node, dependencies] of graph) {
    const orderedDependencies = dependencies.filter(
      (dependency) => orderIndex.get(dependency)! < orderIndex.get(node)!
    )
    pendingDependencyCount.set(node, orderedDependencies.length)
    for (const dependency of orderedDependencies) {
      let list = dependents.get(dependency)
      if (list == null) {
        dependents.set(dependency, list = [])
      }
      list.push(node)
    }
  }
  const blocked = new Set<Node>()
  let stopDispatch = false
  let unsettled = graph.size

  const makeReady = (node: Node): void => {
    const concurrency = concurrencyLimit?.(node)
    if (concurrency == null) {
      ready.push(node)
      return
    }
    nodeConcurrencyGroups.set(node, concurrency.group)
    let group = concurrencyGroups.get(concurrency.group)
    if (group == null) {
      concurrencyGroups.set(concurrency.group, group = {
        limit: concurrency.limit,
        reserved: 0,
        waiting: [],
        waitingHead: 0,
      })
    }
    if (group.reserved < group.limit) {
      group.reserved++
      ready.push(node)
    } else {
      group.waiting.push(node)
    }
  }

  const releaseConcurrency = (node: Node): void => {
    const groupName = nodeConcurrencyGroups.get(node)
    if (groupName == null) return
    const group = concurrencyGroups.get(groupName)!
    group.reserved--
    if (group.waitingHead < group.waiting.length) {
      group.reserved++
      ready.push(group.waiting[group.waitingHead++])
    }
  }

  for (const [node, count] of pendingDependencyCount) {
    if (count === 0) makeReady(node)
  }

  await new Promise<void>((resolve) => {
    const settleIfDone = (): void => {
      // Task runs may opt out because a watch-style script never finishes.
      // Command pipelines retain their prior Promise.all behavior by waiting
      // for work that was already dispatched.
      if (unsettled === 0 || (stopDispatch && (opts.finishInFlight === false || active === 0))) {
        resolve()
      }
    }
    const complete = (node: Node): void => {
      unsettled--
      for (const dependent of dependents.get(node) ?? []) {
        const remaining = pendingDependencyCount.get(dependent)! - 1
        pendingDependencyCount.set(dependent, remaining)
        if (remaining === 0 && !blocked.has(dependent)) {
          makeReady(dependent)
        }
      }
    }
    // A failed task's transitive dependents can never become ready (their
    // dependency count never reaches zero), so they are settled here as
    // skipped instead.
    const block = (node: Node): void => {
      const stack = [node]
      while (stack.length > 0) {
        for (const dependent of dependents.get(stack.pop()!) ?? []) {
          if (blocked.has(dependent)) continue
          blocked.add(dependent)
          unsettled--
          opts.onNodeSkipped(dependent)
          stack.push(dependent)
        }
      }
    }
    const settle = (node: Node, completion: TaskCompletion): void => {
      switch (completion) {
        case 'passed':
          complete(node)
          break
        case 'failed':
          if (opts.bail) {
            unsettled--
            stopDispatch = true
          } else if (opts.continueOnFailure === true) {
            complete(node)
          } else {
            unsettled--
            block(node)
          }
          break
        case 'aborted':
          unsettled--
          stopDispatch = true
          break
      }
    }
    // An explicit queue rather than recursion: a workspace-long chain of
    // pass-through tasks completes synchronously, and call depth must not
    // grow with chain length. Drained by index: shift() moves every
    // remaining element, which is quadratic over a workspace-sized queue.
    let head = 0
    let active = 0
    let pumping = false
    const pump = (): void => {
      if (pumping) return
      pumping = true
      while (!stopDispatch && active < concurrency && head < ready.length) {
        const node = ready[head++]
        active++
        opts.runNode(node).then((completion) => {
          active--
          releaseConcurrency(node)
          settle(node, completion)
          pump()
        }, (error: unknown) => {
          active--
          releaseConcurrency(node)
          // runTask's contract is to never reject; treated as an abort, and
          // the error resurfaces once the scheduler settles.
          if (!rejected) {
            rejected = true
            contractViolation = error
          }
          settle(node, 'aborted')
          pump()
        })
      }
      pumping = false
      settleIfDone()
    }
    pump()
  })
  if (rejected) {
    throw contractViolation
  }
}

function normalizeConcurrency (concurrency: number | undefined): number {
  if (concurrency === Infinity || concurrency == null) return Infinity
  return Number.isInteger(concurrency) && concurrency > 0 ? concurrency : 1
}
