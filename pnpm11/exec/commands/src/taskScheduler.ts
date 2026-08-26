import type { TaskGraph, TaskKey, TaskNode } from './taskGraph.js'

export interface ScheduleTasksOptions {
  /** When `true`, the first failure stops further dispatch; tasks already running finish. */
  bail: boolean
  /**
   * Runs one task's work and resolves with whether it passed. Never rejects:
   * the caller records its own failure details and answers `false`. Not
   * called for pass-through tasks (no scripts to run).
   */
  runTask: (node: TaskNode, key: TaskKey) => Promise<boolean>
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
 * progress: all settled, or — under `bail` after a failure — all in-flight
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
    const fail = (key: TaskKey): void => {
      unsettled--
      if (opts.bail) {
        stopDispatch = true
      } else {
        block(key)
      }
    }
    // An explicit queue rather than recursion: a workspace-long chain of
    // pass-through tasks completes synchronously, and call depth must not
    // grow with chain length.
    let pumping = false
    const pump = (): void => {
      if (pumping) return
      pumping = true
      while (!stopDispatch && ready.length > 0) {
        const key = ready.shift()!
        const node = graph.get(key)!
        if (node.scripts.length === 0) {
          opts.onTaskSkipped(node, key)
          complete(key)
          continue
        }
        inFlight++
        opts.runTask(node, key).then((passed) => {
          inFlight--
          if (passed) {
            complete(key)
          } else {
            fail(key)
          }
          pump()
        }, () => {
          // runTask's contract is to never reject; nothing sane can be done
          // with a rejection here beyond treating it as the failure it is.
          inFlight--
          fail(key)
          pump()
        })
      }
      pumping = false
      settleIfDone()
    }
    pump()
  })
}
