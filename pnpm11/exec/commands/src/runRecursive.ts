import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type { PackageScripts, ProjectRootDir, WorkspaceTasks } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import {
  buildTaskGraph,
  renderTaskGraphDryRun,
  resumeTaskGraphFrom,
  reverseTaskGraph,
  sequenceTasks,
  type TaskGraph,
  taskGraphToJson,
  type TaskKey,
} from '@pnpm/workspace.task-scheduler'

import { throwOrFilterHiddenScripts } from './hiddenScripts.js'
import { tryBuildRegExpFromCommand } from './regexpCommand.js'
import { getRunScriptCommands } from './runScript.js'
import { noRequestedScriptError, runTaskGraph } from './runTaskGraph.js'
import { taskRunExecutionSettings, TaskRunStateContext } from './taskRunState.js'

export type RecursiveRunOpts = Pick<Config,
| 'bin'
| 'enablePrePostScripts'
| 'unsafePerm'
| 'pnpmHomeDir'
| 'requiredScripts'
| 'userAgent'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'shellEmulator'
| 'stream'
| 'syncInjectedDepsAfterScripts'
| 'workspaceDir'
| 'nodeExperimentalPackageMap'
| 'nodeOptions'
| 'modulesDir'
> & Pick<ConfigContext, 'rootProjectManifest' | 'allProjectsGraph' | 'prodAllProjectsGraph' | 'prodOnlySelectedProjectDirs'> & Required<Pick<ConfigContext, 'allProjects' | 'selectedProjectsGraph'> & Pick<Config, 'workspaceDir' | 'dir'>> &
Partial<Pick<Config, 'extendNodePath' | 'extraBinPaths' | 'extraEnv' | 'preferSymlinkedExecutables' | 'bail' | 'dryRun' | 'ignoreWorkspaceCycles' | 'loglevel' | 'reporter' | 'reverse' | 'sort' | 'tasks' | 'workspaceConcurrency'>> &
{
  ifPresent?: boolean
  json?: boolean
  resumeFrom?: string
  reportSummary?: boolean
  sequential?: boolean
}

export async function runRecursive (
  params: string[],
  opts: RecursiveRunOpts
): Promise<string | undefined> {
  if (opts.sequential) {
    opts.workspaceConcurrency = 1
  }
  const [scriptName, ...passedThruArgs] = params
  if (!scriptName) {
    throw new PnpmError('SCRIPT_NAME_IS_REQUIRED', 'You must specify the script you want to run')
  }

  const modulesDirFor = createProjectModulesDirResolver(opts)
  const fullTaskGraph = buildRunTaskGraph(scriptName, opts)
  // Read before sequencing, which drops the edges of a tolerated cycle.
  const hiddenScriptExemptions = dependsOnTargets(fullTaskGraph, {
    reversed: Boolean(opts.reverse),
    tasks: runTasks(opts),
  })
  const taskRunStateContext = createTaskRunStateContext(params, opts, fullTaskGraph)
  const taskGraph = await resumeRunTaskGraph(fullTaskGraph, { scriptName, opts, taskRunStateContext })
  // Also the cycle check: a cyclic graph cannot be scheduled, and sequenced
  // into an arbitrary order it would succeed or fail by luck.
  const sequencedTasks = sequenceTasks(taskGraph, {
    workspaceDir: opts.workspaceDir,
    ignoreCycles: opts.ignoreWorkspaceCycles,
  })

  if (opts.dryRun) {
    return opts.json
      ? JSON.stringify(taskGraphToJson(taskGraph, opts.workspaceDir), null, 2)
      : renderTaskGraphDryRun(taskGraph, sequencedTasks, opts.workspaceDir)
  }

  checkRequestedScripts(taskGraph, { scriptName, opts, hiddenScriptExemptions })
  const taskRunState = await taskRunStateContext.start(collectTasksOutsideGraph(fullTaskGraph, taskGraph))
  return runTaskGraph({ opts, scriptName, passedThruArgs, taskGraph, sequencedTasks, taskRunState, modulesDirFor })
}

function createTaskRunStateContext (params: string[], opts: RecursiveRunOpts, fullTaskGraph: TaskGraph): TaskRunStateContext {
  return new TaskRunStateContext({
    command: 'run',
    params,
    settings: [
      ...taskRunExecutionSettings(opts),
      `enable-pre-post-scripts=${Boolean(opts.enablePrePostScripts)}`,
      `script-shell=${opts.scriptShell ?? ''}`,
      `scripts-prepend-node-path=${String(opts.scriptsPrependNodePath ?? false)}`,
      `shell-emulator=${Boolean(opts.shellEmulator)}`,
      `sync-injected-deps-after-scripts=${JSON.stringify([...(opts.syncInjectedDepsAfterScripts ?? [])].sort())}`,
    ],
    graph: fullTaskGraph,
    workspaceDir: opts.workspaceDir,
    scriptCommands: (node, script) => getRunScriptCommands(
      opts.selectedProjectsGraph[node.project].package.manifest,
      script,
      Boolean(opts.enablePrePostScripts)
    ),
  })
}

async function resumeRunTaskGraph (
  fullTaskGraph: TaskGraph,
  { scriptName, opts, taskRunStateContext }: { scriptName: string, opts: RecursiveRunOpts, taskRunStateContext: TaskRunStateContext }
): Promise<TaskGraph> {
  if (opts.resumeFrom == null) return fullTaskGraph
  const resumeOptions = {
    resumeFrom: opts.resumeFrom,
    selectedProjectsGraph: opts.selectedProjectsGraph,
    taskName: scriptName,
  }
  const taskGraph = resumeTaskGraphFrom(fullTaskGraph, resumeOptions)
  const completedTasks = await taskRunStateContext.readCompletedTasks()
  if (completedTasks == null) return taskGraph
  return resumeTaskGraphFrom(fullTaskGraph, { ...resumeOptions, completedTasks })
}

function checkRequestedScripts (
  taskGraph: TaskGraph,
  { scriptName, opts, hiddenScriptExemptions }: { scriptName: string, opts: RecursiveRunOpts, hiddenScriptExemptions: Set<TaskKey> }
): void {
  throwIfRequiredScriptIsMissing(taskGraph, scriptName, opts)

  if (!process.env.npm_lifecycle_event) {
    filterHiddenRequestedScripts(taskGraph, scriptName, hiddenScriptExemptions)
  }

  // Before anything is dispatched: when no selected project has the script,
  // the run is a user error, and the tasks `dependsOn` pulled in must not
  // have run their side effects by the time it is reported.
  if (scriptName !== 'test' && !opts.ifPresent && [...taskGraph.values()].every((node) => !node.requested || node.scripts.length === 0)) {
    throw noRequestedScriptError(scriptName, opts)
  }
}

function throwIfRequiredScriptIsMissing (taskGraph: TaskGraph, scriptName: string, opts: RecursiveRunOpts): void {
  const requiredScripts = opts.requiredScripts ?? []
  if (!requiredScripts.includes(scriptName)) return
  const missingScriptPackages: string[] = [...taskGraph.values()]
    .filter((node) => node.requested && node.scripts.length === 0)
    .map((node) => {
      const manifest = opts.selectedProjectsGraph[node.project].package.manifest
      return manifest.name ?? node.project
    })
  if (missingScriptPackages.length) {
    throw new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `Missing script "${scriptName}" in packages: ${missingScriptPackages.join(', ')}`)
  }
}

function collectTasksOutsideGraph (fullTaskGraph: TaskGraph, taskGraph: TaskGraph): Set<TaskKey> {
  const tasksOutsideGraph = new Set<TaskKey>()
  for (const key of fullTaskGraph.keys()) {
    if (!taskGraph.has(key)) tasksOutsideGraph.add(key)
  }
  return tasksOutsideGraph
}

/**
 * The task graph of one `pnpm -r run` invocation: `scriptName` in every
 * selected project plus what `dependsOn` pulls in, with `--reverse` applied.
 *
 * `--no-sort` keeps its meaning of disregarding ordering entirely: tasks get
 * no edges, and the `tasks` declarations do not apply.
 */
function buildRunTaskGraph (scriptName: string, opts: RecursiveRunOpts): TaskGraph {
  const projectDependencies = opts.sort
    ? filteredProjectsDependencies(opts)
    : new Map((Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort().map((project) => [project, [] as ProjectRootDir[]]))
  if (!opts.sort && opts.tasks != null && Object.keys(opts.tasks).length > 0) {
    globalWarn('The tasks declarations in pnpm-workspace.yaml are ignored because sorting is disabled (--no-sort or --parallel)')
  }
  let taskGraph = buildTaskGraph({
    projectDependencies,
    scriptsByProject: (project) => opts.selectedProjectsGraph[project].package.manifest.scripts ?? {},
    selectScripts: getSpecifiedScripts,
    taskName: scriptName,
    tasks: runTasks(opts),
    isSelectorTaskName: isRegExpSelector,
  })
  if (opts.reverse) {
    taskGraph = reverseTaskGraph(taskGraph)
  }
  return taskGraph
}

export function getSpecifiedScripts (scripts: PackageScripts, scriptName: string): string[] {
  // if scripts in package.json has script which is equal to scriptName a user passes, return it.
  if (scripts[scriptName]) {
    return [scriptName]
  }

  const scriptSelector = tryBuildRegExpFromCommand(scriptName)

  // if scriptName which a user passes is RegExp (like /build:.*/), multiple scripts to execute will be selected with RegExp
  if (scriptSelector) {
    return Object.keys(scripts).filter(script => Boolean(scripts[script]) && scriptSelector.test(script))
  }

  return []
}

/**
 * Removes hidden scripts from the requested tasks, and throws when everything
 * a project's requested tasks select is hidden. Checked only for the tasks
 * the invocation named: a `dependsOn` declaration naming a hidden script is a
 * deliberate reference, like a call from another script, so the `exempt`
 * tasks such a declaration targets are not checked. Checked per project, over
 * every requested task: a RegExp selector can seed one task per matched
 * script.
 */
function filterHiddenRequestedScripts (taskGraph: TaskGraph, scriptName: string, exempt: Set<TaskKey>): void {
  const checkedNodes = [...taskGraph].filter(([key, node]) => node.requested && !exempt.has(key)).map(([, node]) => node)
  const requestedScriptsByProject = new Map<string, string[]>()
  for (const node of checkedNodes) {
    const scripts = requestedScriptsByProject.get(node.project) ?? []
    scripts.push(...node.scripts)
    requestedScriptsByProject.set(node.project, scripts)
  }
  const visibleScriptsByProject = new Map<string, Set<string>>()
  for (const [project, scripts] of requestedScriptsByProject) {
    visibleScriptsByProject.set(project, new Set(throwOrFilterHiddenScripts(scripts, scriptName)))
  }
  for (const node of checkedNodes) {
    const visibleScripts = visibleScriptsByProject.get(node.project)!
    node.scripts = node.scripts.filter((script) => visibleScripts.has(script))
  }
}

/**
 * The tasks a `dependsOn` declaration targets: every dependency of a task that
 * has a `tasks` entry. A task without one only has the default dependency on
 * its own name in the dependency projects, which is no reference. Edges are
 * read in declaration direction when `--reverse` inverted them.
 */
function dependsOnTargets (graph: TaskGraph, opts: { reversed: boolean, tasks: WorkspaceTasks | undefined }): Set<TaskKey> {
  const targets = new Set<TaskKey>()
  if (opts.tasks == null) return targets
  const tasks = opts.tasks
  for (const [key, node] of graph) {
    for (const dependency of node.dependencies) {
      const [dependent, target] = opts.reversed ? [dependency, key] : [key, dependency]
      if (Object.hasOwn(tasks, graph.get(dependent)!.taskName)) targets.add(target)
    }
  }
  return targets
}

/** The `tasks` declarations the run's graph follows: none under `--no-sort`. */
function runTasks (opts: Pick<RecursiveRunOpts, 'sort' | 'tasks'>): WorkspaceTasks | undefined {
  return opts.sort ? opts.tasks : undefined
}

/**
 * Whether a task name addresses scripts by RegExp literal rather than by
 * name. A selector carrying flags is shaped like one but is rejected by
 * `tryBuildRegExpFromCommand`; it is a selector here too, so the graph
 * treats it the way `getSpecifiedScripts` does.
 */
function isRegExpSelector (taskName: string): boolean {
  try {
    return tryBuildRegExpFromCommand(taskName) != null
  } catch {
    return true
  }
}
