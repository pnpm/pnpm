import type { TaskGraph, TaskKey, TaskNode } from './taskGraph.js'

/** How the scheduler saw one task end. */
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
  /** When `true`, the first failure stops further dispatch; tasks already running finish. */
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

/**
 * Dispatches every task whose dependencies have all completed successfully,
 * in dependency order and nothing else — concurrency among ready tasks is the
 * caller's to limit inside `runTask`. Resolves once no task can make further
 * progress: all settled, or — after a bail or an abort — all in-flight
 * work finished. Tasks never dispatched are left untouched, so their
 * caller-side status stays whatever "queued" is.
 *
 * The graph must be acyclic ({@link sequenceTasks} proves it); a cycle would
 * hang this scheduler.
 */
export async function scheduleTasks (graph: TaskGraph, opts: ScheduleTasksOptions): Promise<void> {
  const pendingDependencyCount = new Map<TaskKey, number>()
  const dependents = new Map<TaskKey, TaskKey[]>()
  const ready: TaskKey[] = []
  for (const [key, node] of graph) {
    pendingDependencyCount.set(key, node.dependencies.length)
    if (node.dependencies.length === 0) {
      ready.push(key)
    }
    for (const dependency of node.dependencies) {
      let list = dependents.get(dependency)
      if (list == null) {
        dependents.set(dependency, list = [])
      }
      list.push(key)
    }
  }
  const blocked = new Set<TaskKey>()
  let stopDispatch = false
  let unsettled = graph.size
  let inFlight = 0

  await new Promise<void>((resolve) => {
    const settleIfDone = (): void => {
      if (unsettled === 0 || (stopDispatch && inFlight === 0)) {
        resolve()
      }
    }
    const complete = (key: TaskKey): void => {
      unsettled--
      for (const dependent of dependents.get(key) ?? []) {
        const remaining = pendingDependencyCount.get(dependent)! - 1
        pendingDependencyCount.set(dependent, remaining)
        if (remaining === 0) {
          ready.push(dependent)
        }
      }
    }
    // A failed task's transitive dependents can never become ready (their
    // dependency count never reaches zero), so they are settled here as
    // skipped instead.
    const block = (key: TaskKey): void => {
      const stack = [key]
      while (stack.length > 0) {
        for (const dependent of dependents.get(stack.pop()!) ?? []) {
          if (blocked.has(dependent)) continue
          blocked.add(dependent)
          unsettled--
          opts.onTaskSkipped(graph.get(dependent)!, dependent)
          stack.push(dependent)
        }
      }
    }
    const settle = (key: TaskKey, completion: TaskCompletion): void => {
      switch (completion) {
        case 'passed':
          complete(key)
          break
        case 'failed':
          unsettled--
          if (opts.bail) {
            stopDispatch = true
          } else {
            block(key)
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
    let pumping = false
    const pump = (): void => {
      if (pumping) return
      pumping = true
      while (!stopDispatch && head < ready.length) {
        const key = ready[head++]
        const node = graph.get(key)!
        if (node.scripts.length === 0) {
          opts.onTaskSkipped(node, key)
          complete(key)
          continue
        }
        inFlight++
        opts.runTask(node, key).then((completion) => {
          inFlight--
          settle(key, completion)
          pump()
        }, () => {
          // runTask's contract is to never reject; a rejection is an
          // infrastructure failure whose details only the caller can hold.
          inFlight--
          settle(key, 'aborted')
          pump()
        })
      }
      pumping = false
      settleIfDone()
    }
    pump()
  })
}
