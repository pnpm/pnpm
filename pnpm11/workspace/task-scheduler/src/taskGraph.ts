import path from 'node:path'

import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { PackageScripts, ProjectRootDir, ProjectsGraph, WorkspaceTasks } from '@pnpm/types'

/**
 * A task is a `(project, task name)` pair; its key is the stable identifier
 * used by the scheduler, summaries, and dry-run output.
 */
export type TaskKey = string

export interface TaskNode {
  project: ProjectRootDir
  taskName: string
  concurrency?: number
  /**
   * The scripts of the project that the task name selected — several when the
   * task name is a RegExp selector. Empty when the project has no such
   * script: the task is then a pass-through that runs nothing, completes as
   * soon as its dependencies have, and is reported as skipped, so that a
   * scriptless project does not sever a dependency chain.
   */
  scripts: string[]
  /** Whether the invocation named this task, as opposed to `dependsOn` pulling it in. */
  requested: boolean
  dependencies: TaskKey[]
}

export type TaskGraph = Map<TaskKey, TaskNode>

export interface BuildTaskGraphOptions {
  /**
   * The dependency edges among the selected projects, already resolved
   * through the full workspace graph (`filteredProjectsDependencies`). Tasks
   * are created only for these projects: `dependsOn` never runs anything in a
   * project the filter did not select.
   */
  projectDependencies: Map<ProjectRootDir, ProjectRootDir[]>
  scriptsByProject: (project: ProjectRootDir) => PackageScripts
  selectScripts: (scripts: PackageScripts, taskName: string) => string[]
  /**
   * The script the invocation runs; every selected project gets a task named
   * this, or one per matched script for an expanded selector.
   */
  taskName: string
  tasks?: WorkspaceTasks
  /**
   * Whether a task name is a RegExp selector (a `/pattern/` literal). With
   * `tasks` declared, a selector seeds one task per script `selectScripts`
   * matches in each project, so each resolves the `dependsOn` of its own name.
   */
  isSelectorTaskName?: (taskName: string) => boolean
}

/**
 * Builds the graph of tasks the invocation runs: a task named `taskName` in
 * every selected project, plus every task those transitively pull in through
 * `dependsOn`. A task with no `tasks` entry behaves as
 * `dependsOn: ['^<its own name>']`: plain topological order over the
 * project graph.
 */
interface QueueItem {
  project: ProjectRootDir
  taskName: string
  requested: boolean
}

export function buildTaskGraph (opts: BuildTaskGraphOptions): TaskGraph {
  const graph: TaskGraph = new Map()
  const queue: QueueItem[] = seedTaskQueue(opts)
  let head = 0
  while (head < queue.length) {
    const item = queue[head++]
    const key = taskKey(item.project, item.taskName)
    const existing = graph.get(key)
    if (existing != null) {
      existing.requested ||= item.requested
      continue
    }
    const scripts = opts.selectScripts(opts.scriptsByProject(item.project), item.taskName)
    const dependencies = resolveTaskDependencies(opts, item, scripts, queue)
    graph.set(key, {
      project: item.project,
      taskName: item.taskName,
      concurrency: taskConcurrency(opts.tasks, item.taskName),
      scripts,
      requested: item.requested,
      dependencies,
    })
  }
  return graph
}

function seedTaskQueue (opts: BuildTaskGraphOptions): QueueItem[] {
  const queue: QueueItem[] = []
  const expandsSelector = expandsSelectorTask(opts)
  for (const project of opts.projectDependencies.keys()) {
    const matchedScripts = expandsSelector
      ? opts.selectScripts(opts.scriptsByProject(project), opts.taskName)
      : []
    const seedTaskNames = matchedScripts.length > 0 ? matchedScripts : [opts.taskName]
    for (const taskName of seedTaskNames) {
      queue.push({ project, taskName, requested: true })
    }
  }
  return queue
}

function resolveTaskDependencies (
  opts: BuildTaskGraphOptions,
  item: QueueItem,
  scripts: string[],
  queue: QueueItem[]
): TaskKey[] {
  const expandsSelector = expandsSelectorTask(opts)
  const isSkippedPassThrough = expandsSelector && item.taskName === opts.taskName && scripts.length === 0
  const dependsOn = isSkippedPassThrough ? [] : taskDependsOn(opts.tasks, item.taskName)
  const dependencies = new Set<TaskKey>()
  for (const entry of dependsOn) {
    if (entry.startsWith('^')) {
      const depName = entry.slice(1)
      for (const depProject of opts.projectDependencies.get(item.project) ?? []) {
        dependencies.add(taskKey(depProject, depName))
        queue.push({ project: depProject, taskName: depName, requested: false })
      }
    } else {
      dependencies.add(taskKey(item.project, entry))
      queue.push({ project: item.project, taskName: entry, requested: false })
    }
  }
  return [...dependencies]
}


function taskConcurrency (tasks: WorkspaceTasks | undefined, taskName: string): number | undefined {
  return tasks != null && Object.hasOwn(tasks, taskName)
    ? tasks[taskName].concurrency
    : undefined
}

export function taskKey (project: ProjectRootDir, taskName: string): TaskKey {
  return `${project}\0${taskName}`
}

/**
 * The `dependsOn` entries of `taskName`. An own-property check, not a plain
 * lookup: a script named like an `Object.prototype` member (`constructor`,
 * `toString`, ...) must get the default rather than resolve an inherited
 * value.
 */
function taskDependsOn (tasks: WorkspaceTasks | undefined, taskName: string): string[] {
  if (tasks != null && Object.hasOwn(tasks, taskName)) {
    return tasks[taskName].dependsOn ?? []
  }
  return [`^${taskName}`]
}

/**
 * Whether the invocation's RegExp selector expands into a task per matched
 * script. Only with `tasks` declared, and not when an exact `tasks` entry
 * under the selector string itself governs it as a single task.
 */
function expandsSelectorTask (opts: BuildTaskGraphOptions): boolean {
  return opts.tasks != null &&
    Object.keys(opts.tasks).length > 0 &&
    !Object.hasOwn(opts.tasks, opts.taskName) &&
    opts.isSelectorTaskName?.(opts.taskName) === true
}

export interface SequenceTasksOptions {
  workspaceDir: string
  /**
   * The `ignoreWorkspaceCycles` setting: the workspace has declared its
   * cycles deliberate, so a cyclic task graph is downgraded from an error
   * to a warning, backward edges are dropped, and the members run in the
   * graph sequencer's deterministic order.
   */
  ignoreCycles?: boolean
}

/**
 * Topologically orders the task graph, throwing when the tasks form a cycle
 * unless `ignoreCycles` tolerates it and mutates the graph's edges acyclic.
 * Detection is scoped to this graph: a cycle among tasks the filter did not
 * select cannot fail the run.
 */
export function sequenceTasks (graph: TaskGraph, opts: SequenceTasksOptions): TaskKey[] {
  const edges = new Map<TaskKey, TaskKey[]>()
  for (const [key, node] of graph) {
    edges.set(key, node.dependencies)
  }
  const result = graphSequencer(edges, [...graph.keys()])
  if (result.cycles.length > 0) {
    const cycles = result.cycles.map((cycle) =>
      [...cycle, cycle[0]].map((key) => formatTask(graph.get(key)!, opts.workspaceDir)).join(' → ')
    ).join('; ')
    if (!opts.ignoreCycles) {
      throw new PnpmError('TASK_CYCLE', `The tasks form a dependency cycle: ${cycles}`, {
        hint: 'If the cycles are deliberate, set ignoreWorkspaceCycles to true to run their tasks in an arbitrary order.',
      })
    }
    globalWarn(`The tasks form a dependency cycle and run in an arbitrary order relative to each other because ignoreWorkspaceCycles is set: ${cycles}`)
    dropCyclicDependencies(graph, result.order)
  }
  return result.order
}

/**
 * Keeps only dependencies that point backward in the sequencer's order,
 * making an ignored cyclic graph deterministic and runnable.
 */
function dropCyclicDependencies (graph: TaskGraph, order: TaskKey[]): void {
  const orderIndex = new Map(order.map((key, index) => [key, index]))
  for (const [key, node] of graph) {
    node.dependencies = node.dependencies.filter(
      (dependency) => orderIndex.get(dependency)! < orderIndex.get(key)!
    )
  }
}

export function formatTask (node: TaskNode, workspaceDir: string): string {
  return `${relativeProjectDir(node.project, workspaceDir)}#${node.taskName}`
}

function relativeProjectDir (project: ProjectRootDir, workspaceDir: string): string {
  const relative = path.relative(workspaceDir, project)
  return relative === '' ? '.' : relative.replaceAll(path.sep, '/')
}

/** The same graph with every edge turned around: dependents run before dependencies. */
export function reverseTaskGraph (graph: TaskGraph): TaskGraph {
  const reversed: TaskGraph = new Map()
  for (const [key, node] of graph) {
    reversed.set(key, { ...node, dependencies: [] })
  }
  for (const [key, node] of graph) {
    for (const dependency of node.dependencies) {
      reversed.get(dependency)!.dependencies.push(key)
    }
  }
  return reversed
}

export interface ResumeTaskGraphOptions {
  resumeFrom: string
  selectedProjectsGraph: ProjectsGraph
  /**
   * The task of the anchor project the invocation resolves to. When a RegExp
   * selector expanded into per-script tasks, every requested task of the
   * anchor project is an anchor.
   */
  taskName: string
  /** Tasks durably completed by the matching previous invocation. */
  completedTasks?: ReadonlySet<TaskKey>
}

/**
 * When durable state is available, the graph without exactly those completed
 * tasks. Otherwise, the graph without the anchor's transitive dependencies —
 * the tasks inferred to have finished before a run would reach the anchor.
 * The anchor itself and unfinished work stay, and edges into the dropped set
 * are treated as satisfied.
 */
export function resumeTaskGraphFrom (graph: TaskGraph, opts: ResumeTaskGraphOptions): TaskGraph {
  const anchorProject = (Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[])
    .find((project) => opts.selectedProjectsGraph[project]?.package.manifest.name === opts.resumeFrom)
  if (!anchorProject) {
    throw new PnpmError('RESUME_FROM_NOT_FOUND', `Cannot find package ${opts.resumeFrom}. Could not determine where to resume from.`)
  }
  const anchorKeys = resumeAnchorKeys(graph, anchorProject, opts.taskName)
  if (anchorKeys.size === 0) {
    // The anchor exists but its task is not in this graph (e.g. a
    // non-recursive invocation): there is nothing to skip.
    return graph
  }
  const dropped = opts.completedTasks == null
    ? transitiveDependencies(graph, anchorKeys)
    : new Set([...opts.completedTasks].filter((key) => !anchorKeys.has(key) && graph.has(key)))
  const resumed: TaskGraph = new Map()
  for (const [key, node] of graph) {
    if (dropped.has(key)) continue
    resumed.set(key, { ...node, dependencies: node.dependencies.filter((dependency) => !dropped.has(dependency)) })
  }
  return resumed
}

/**
 * The anchor project's task for `taskName`, or, when a RegExp selector
 * expanded into a task per matched script, every task requested in that
 * project.
 */
function resumeAnchorKeys (graph: TaskGraph, anchorProject: ProjectRootDir, taskName: string): Set<TaskKey> {
  const key = taskKey(anchorProject, taskName)
  if (graph.has(key)) return new Set([key])
  return new Set([...graph].filter(([, node]) => node.requested && node.project === anchorProject).map(([key]) => key))
}

/** The anchors' transitive dependencies, other than the anchors themselves. */
function transitiveDependencies (graph: TaskGraph, anchorKeys: Set<TaskKey>): Set<TaskKey> {
  const dependencies = new Set<TaskKey>()
  const stack = [...anchorKeys].flatMap((key) => graph.get(key)!.dependencies)
  while (stack.length > 0) {
    const key = stack.pop()!
    if (dependencies.has(key)) continue
    dependencies.add(key)
    stack.push(...graph.get(key)!.dependencies)
  }
  for (const key of anchorKeys) dependencies.delete(key)
  return dependencies
}

/**
 * Whether at most one script can ever be in flight, which is when output may
 * stay inherited rather than piped: no task runs several scripts at once, and
 * every script-running task lies on one dependency chain, so the graph forces
 * them to run one after another.
 *
 * `sequencedTasks` is {@link sequenceTasks}'s result — the proof the graph is
 * acyclic, and the evaluation order for the longest-chain scan.
 */
export function isSerialTaskGraph (graph: TaskGraph, sequencedTasks: TaskKey[]): boolean {
  let scriptTaskCount = 0
  for (const node of graph.values()) {
    if (node.scripts.length > 1) return false
    scriptTaskCount += node.scripts.length
  }
  if (scriptTaskCount <= 1) return true
  const chainLength = new Map<TaskKey, number>()
  let longestChain = 0
  for (const key of sequencedTasks) {
    const node = graph.get(key)!
    let viaDependencies = 0
    for (const dependency of node.dependencies) {
      viaDependencies = Math.max(viaDependencies, chainLength.get(dependency) ?? 0)
    }
    const length = viaDependencies + node.scripts.length
    chainLength.set(key, length)
    longestChain = Math.max(longestChain, length)
  }
  return longestChain === scriptTaskCount
}

export interface DryRunTaskDependency {
  project: string
  script: string
}

export interface DryRunTask extends DryRunTaskDependency {
  missingScript: boolean
  dependsOn: DryRunTaskDependency[]
}

/**
 * What `--dry-run --json` emits: nodes and edges rather than an order, since
 * independent tasks have no required sequence. Identifiers are the
 * workspace-relative project directory and the script name.
 */
export function taskGraphToJson (graph: TaskGraph, workspaceDir: string): { tasks: DryRunTask[] } {
  const tasks = [...graph.values()]
    .map((node) => ({
      project: relativeProjectDir(node.project, workspaceDir),
      script: node.taskName,
      missingScript: node.scripts.length === 0,
      dependsOn: node.dependencies
        .map((dependency) => {
          const dependencyNode = graph.get(dependency)!
          return {
            project: relativeProjectDir(dependencyNode.project, workspaceDir),
            script: dependencyNode.taskName,
          }
        })
        .sort(compareTaskIds),
    }))
    .sort(compareTaskIds)
  return { tasks }
}

function compareTaskIds (left: DryRunTaskDependency, right: DryRunTaskDependency): number {
  return lexCompare(left.project, right.project) || lexCompare(left.script, right.script)
}

/**
 * What plain `--dry-run` prints: one valid linearization of the graph — not
 * the order the scheduler will follow. Ties among simultaneously runnable
 * tasks are broken by project directory, so two dry runs of one workspace
 * print the same thing and their diff is meaningful.
 */
export function renderTaskGraphDryRun (graph: TaskGraph, sequencedTasks: TaskKey[], workspaceDir: string): string {
  return sequencedTasks.map((key) => {
    const node = graph.get(key)!
    return node.scripts.length === 0
      ? `${formatTask(node, workspaceDir)} (skipped: no such script)`
      : formatTask(node, workspaceDir)
  }).join('\n')
}
