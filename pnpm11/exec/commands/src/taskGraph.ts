import path from 'node:path'

import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { PnpmError } from '@pnpm/error'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { PackageScripts, ProjectRootDir, ProjectsGraph, WorkspaceTasks } from '@pnpm/types'

/**
 * A task is a `(project, task name)` pair; its key is the stable identifier
 * the scheduler, the summary, and the dry-run output agree on.
 */
export type TaskKey = string

export interface TaskNode {
  project: ProjectRootDir
  taskName: string
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
  /** The script the invocation runs; every selected project gets a task named this. */
  taskName: string
  tasks?: WorkspaceTasks
}

/**
 * Builds the graph of tasks the invocation runs: a task named `taskName` in
 * every selected project, plus every task those transitively pull in through
 * `dependsOn`. A task with no `tasks` entry behaves as
 * `dependsOn: ['^<its own name>']`: plain topological order over the
 * project graph.
 */
export function buildTaskGraph (opts: BuildTaskGraphOptions): TaskGraph {
  const graph: TaskGraph = new Map()
  const queue: Array<{ project: ProjectRootDir, taskName: string, requested: boolean }> = []
  for (const project of opts.projectDependencies.keys()) {
    queue.push({ project, taskName: opts.taskName, requested: true })
  }
  // Drained by index: shift() moves every remaining element, which is
  // quadratic over a workspace-sized queue.
  let head = 0
  while (head < queue.length) {
    const { project, taskName, requested } = queue[head++]
    const key = taskKey(project, taskName)
    const existing = graph.get(key)
    if (existing != null) {
      existing.requested ||= requested
      continue
    }
    const dependencies = new Set<TaskKey>()
    for (const entry of taskDependsOn(opts.tasks, taskName)) {
      if (entry.startsWith('^')) {
        const dependencyTaskName = entry.slice(1)
        for (const dependencyProject of opts.projectDependencies.get(project) ?? []) {
          dependencies.add(taskKey(dependencyProject, dependencyTaskName))
          queue.push({ project: dependencyProject, taskName: dependencyTaskName, requested: false })
        }
      } else {
        dependencies.add(taskKey(project, entry))
        queue.push({ project, taskName: entry, requested: false })
      }
    }
    graph.set(key, {
      project,
      taskName,
      scripts: opts.selectScripts(opts.scriptsByProject(project), taskName),
      requested,
      dependencies: [...dependencies],
    })
  }
  return graph
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
 * Topologically sequences the task graph into ready-together groups —
 * dependencies always in an earlier group — throwing when the tasks form a
 * cycle. Detection is scoped to this graph: a cycle among tasks the filter
 * did not select cannot fail the run.
 */
export function sequenceTasks (graph: TaskGraph, workspaceDir: string): TaskKey[][] {
  const edges = new Map<TaskKey, TaskKey[]>()
  for (const [key, node] of graph) {
    edges.set(key, node.dependencies)
  }
  const result = graphSequencer(edges, [...graph.keys()])
  if (result.cycles.length > 0) {
    const cycles = result.cycles.map((cycle) =>
      [...cycle, cycle[0]].map((key) => formatTask(graph.get(key)!, workspaceDir)).join(' → ')
    )
    throw new PnpmError('TASK_CYCLE', `The tasks form a dependency cycle: ${cycles.join('; ')}`)
  }
  return result.chunks
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
  /** The task of the anchor project the invocation resolves to. */
  taskName: string
}

/**
 * The graph without the anchor's transitive dependencies — the tasks known to
 * have finished before a run would reach the anchor. Everything else stays,
 * including work unrelated to the anchor, and edges into the dropped set are
 * treated as satisfied.
 */
export function resumeTaskGraphFrom (graph: TaskGraph, opts: ResumeTaskGraphOptions): TaskGraph {
  const anchorProject = (Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[])
    .find((project) => opts.selectedProjectsGraph[project]?.package.manifest.name === opts.resumeFrom)
  if (!anchorProject) {
    throw new PnpmError('RESUME_FROM_NOT_FOUND', `Cannot find package ${opts.resumeFrom}. Could not determine where to resume from.`)
  }
  const anchor = graph.get(taskKey(anchorProject, opts.taskName))
  if (anchor == null) {
    // The anchor exists but its task is not in this graph (e.g. a
    // non-recursive invocation): there is nothing to skip.
    return graph
  }
  const dropped = new Set<TaskKey>()
  const stack = [...anchor.dependencies]
  while (stack.length > 0) {
    const key = stack.pop()!
    if (dropped.has(key)) continue
    dropped.add(key)
    stack.push(...graph.get(key)!.dependencies)
  }
  const resumed: TaskGraph = new Map()
  for (const [key, node] of graph) {
    if (dropped.has(key)) continue
    resumed.set(key, { ...node, dependencies: node.dependencies.filter((dependency) => !dropped.has(dependency)) })
  }
  return resumed
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
export function isSerialTaskGraph (graph: TaskGraph, sequencedTasks: TaskKey[][]): boolean {
  let scriptTaskCount = 0
  for (const node of graph.values()) {
    if (node.scripts.length > 1) return false
    scriptTaskCount += node.scripts.length
  }
  if (scriptTaskCount <= 1) return true
  const chainLength = new Map<TaskKey, number>()
  let longestChain = 0
  for (const group of sequencedTasks) {
    for (const key of group) {
      const node = graph.get(key)!
      let viaDependencies = 0
      for (const dependency of node.dependencies) {
        viaDependencies = Math.max(viaDependencies, chainLength.get(dependency) ?? 0)
      }
      const length = viaDependencies + node.scripts.length
      chainLength.set(key, length)
      longestChain = Math.max(longestChain, length)
    }
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
export function renderTaskGraphDryRun (graph: TaskGraph, sequencedTasks: TaskKey[][], workspaceDir: string): string {
  const lines: string[] = []
  for (const group of sequencedTasks) {
    const nodes = group.map((key) => graph.get(key)!)
    nodes.sort((left, right) => lexCompare(left.project, right.project) || lexCompare(left.taskName, right.taskName))
    for (const node of nodes) {
      lines.push(node.scripts.length === 0
        ? `${formatTask(node, workspaceDir)} (skipped: no such script)`
        : formatTask(node, workspaceDir))
    }
  }
  return lines.join('\n')
}
