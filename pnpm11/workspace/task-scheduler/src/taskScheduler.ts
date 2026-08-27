import type { TaskGraph, TaskKey, TaskNode } from './taskGraph.js'

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

/**
 * Dispatches every task whose dependencies have all completed successfully,
 * in dependency order and nothing else — concurrency among ready tasks is the
 * caller's to limit inside `runTask`. Resolves once all tasks settled, or as
 * soon as a bailed failure or an abort stops the run: in-flight work is then
 * abandoned to the caller, whose exit path terminates the running commands.
 * Tasks never dispatched are left untouched, so their caller-side status
 * stays whatever "queued" is.
 *
 * The graph must be acyclic ({@link sequenceTasks} proves it); a cycle would
 * hang this scheduler.
 */
export async function scheduleTasks (graph: TaskGraph, opts: ScheduleTasksOptions): Promise<void> {
  // A rejection violates runTask's contract; held here so the run still
  // fails with it rather than silently resolving. First error wins: a
  // rejection landing only after something else already stopped the run is
  // abandoned along with the rest of the in-flight work, exactly as a
  // second script failure after a bail is.
  let contractViolation: unknown
  let rejected = false
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

  await new Promise<void>((resolve) => {
    const settleIfDone = (): void => {
      // A stopped run resolves without waiting for in-flight tasks: a
      // watch-style script never finishes, and the first failure must not
      // leave the run hanging on it.
      if (unsettled === 0 || stopDispatch) {
        resolve()
      }
    }
    const complete = (key: TaskKey): void => {
      unsettled--
      for (const dependent of dependents.get(key) ?? []) {
        const remaining = pendingDependencyCount.get(dependent)! - 1
        pendingDependencyCount.set(dependent, remaining)
        if (remaining === 0 && !blocked.has(dependent)) {
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
        opts.runTask(node, key).then((completion) => {
          settle(key, completion)
          pump()
        }, (error: unknown) => {
          // runTask's contract is to never reject; treated as an abort, and
          // the error resurfaces once the scheduler settles.
          if (!rejected) {
            rejected = true
            contractViolation = error
          }
          settle(key, 'aborted')
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
