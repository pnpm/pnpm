import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, createProjectModulesDirResolver, getWorkspaceConcurrency, types } from '@pnpm/config.reader'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { keepEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import type { Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import {
  resumeTaskGraphFrom,
  reverseTaskGraph,
  sequenceTasks,
  type TaskGraph,
  type TaskKey,
  taskKey,
} from '@pnpm/workspace.task-scheduler'
import pLimit from 'p-limit'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { runExecTaskGraph } from './execTaskGraph.js'
import { getExecutionDuration, writeRecursiveSummary } from './recursiveSummary.js'
import {
  PARALLEL_OPTION_HELP,
  REPORT_SUMMARY_OPTION_HELP,
  RESUME_FROM_OPTION_HELP,
  shorthands as runShorthands,
} from './run.js'
import { runDepsStatusCheck } from './runDepsStatusCheck.js'
import { taskRunExecutionSettings, TaskRunStateContext } from './taskRunState.js'

export { getExecutionDuration, writeRecursiveSummary }

export const shorthands: Record<string, string | string[]> = {
  parallel: runShorthands.parallel,
  c: '--shell-mode',
}

export const commandNames = ['exec']

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'bail',
      'sort',
      'unsafe-perm',
      'workspace-concurrency',
      'reporter-hide-prefix',
      'node-experimental-package-map',
      'node-package-map-type',
    ], types),
    'shell-mode': Boolean,
    'resume-from': String,
    'report-summary': Boolean,
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  reverse: Boolean,
})

export function help (): string {
  return renderHelp({
    description: 'Run a shell command in the context of a project.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Do not hide project name prefix from output of recursively running command.',
            name: '--no-reporter-hide-prefix',
          },
          PARALLEL_OPTION_HELP,
          {
            description: 'Run the shell command in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'If exist, runs file inside of a shell. \
Uses /bin/sh on UNIX and \\cmd.exe on Windows. \
The shell should understand the -c switch on UNIX or /d /s /c on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
          RESUME_FROM_OPTION_HELP,
          REPORT_SUMMARY_OPTION_HELP,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('exec'),
    usages: ['pnpm [-r] [-c] exec <command> [args...]'],
  })
}

export type ExecOpts = Required<Pick<ConfigContext, 'selectedProjectsGraph'>> & {
  bail?: boolean
  unsafePerm?: boolean
  reverse?: boolean
  sort?: boolean
  workspaceConcurrency?: number
  shellMode?: boolean
  resumeFrom?: string
  reportSummary?: boolean
  implicitlyFellbackFromRun?: boolean
} & Pick<Config,
| 'bin'
| 'dir'
| 'extendNodePath'
| 'extraBinPaths'
| 'extraEnv'
| 'lockfileDir'
| 'loglevel'
| 'modulesDir'
| 'nodeOptions'
| 'nodeExperimentalPackageMap'
| 'pnpmHomeDir'
| 'preferSymlinkedExecutables'
| 'recursive'
| 'reporter'
| 'reporterHidePrefix'
| 'userAgent'
| 'verifyDepsBeforeRun'
| 'workspaceDir'
> & Partial<Pick<Config, 'filter' | 'filterProd'>> & Pick<Config, 'ignoreWorkspaceCycles'> & Pick<ConfigContext, 'cliOptions' | 'allProjectsGraph' | 'prodAllProjectsGraph' | 'prodOnlySelectedProjectDirs'> & Partial<Pick<ConfigContext, 'rawCliConfig'>> & CheckDepsStatusOptions

export async function handler (
  opts: ExecOpts,
  params: string[]
): Promise<{ exitCode: number }> {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  if (!params[0]) {
    throw new PnpmError('EXEC_MISSING_COMMAND', '\'pnpm exec\' requires a command to run')
  }
  const modulesDirFor = createProjectModulesDirResolver(opts)
  const limitRun = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency))

  if (opts.verifyDepsBeforeRun) {
    await runDepsStatusCheck(opts)
  }

  const commandName = params[0]
  const fullTaskGraph = opts.recursive
    ? buildRecursiveExecTaskGraph(opts, commandName)
    : await buildSingleProjectExecTaskGraph(opts, commandName)

  if (!opts.selectedProjectsGraph) {
    throw new PnpmError('RECURSIVE_EXEC_NO_PACKAGE', 'No package found in this workspace')
  }

  const baseExtraEnv: Record<string, string | undefined> = {
    ...opts.extraEnv,
    ...(opts.nodeOptions ? { NODE_OPTIONS: keepEsmNodePathLoaderOption(opts.nodeOptions, opts.extraEnv?.NODE_OPTIONS) } : {}),
  }
  const taskRunStateContext = opts.recursive
    ? createExecTaskRunStateContext({ opts, params, baseExtraEnv, fullTaskGraph })
    : undefined
  const taskGraph = await resumeExecTaskGraph(fullTaskGraph, { opts, commandName, taskRunStateContext })

  // Also the cycle check: a cyclic graph cannot be scheduled, and sequenced
  // into an arbitrary order it would succeed or fail by luck.
  sequenceTasks(taskGraph, {
    workspaceDir: opts.workspaceDir ?? opts.dir,
    ignoreCycles: opts.ignoreWorkspaceCycles,
  })

  const taskRunState = await taskRunStateContext?.start(collectTasksOutsideGraph(fullTaskGraph, taskGraph))
  return runExecTaskGraph({ opts, params, taskGraph, taskRunState, baseExtraEnv, modulesDirFor, limitRun })
}

/**
 * `exec` runs one command per project, so its task graph is one task per
 * selected project over the project dependency edges: it gets the
 * dependency-order scheduling, while `dependsOn` declarations — which name
 * scripts — do not apply to it.
 */
function buildRecursiveExecTaskGraph (opts: ExecOpts, commandName: string): TaskGraph {
  const projectDependencies = opts.sort
    ? filteredProjectsDependencies(opts)
    : new Map((Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort().map((project) => [project, [] as ProjectRootDir[]]))
  let taskGraph: TaskGraph = new Map()
  for (const [project, dependencies] of projectDependencies) {
    taskGraph.set(taskKey(project, commandName), {
      project,
      taskName: commandName,
      scripts: [commandName],
      requested: true,
      dependencies: dependencies.map((dependency) => taskKey(dependency, commandName)),
    })
  }
  if (opts.reverse) {
    taskGraph = reverseTaskGraph(taskGraph)
  }
  return taskGraph
}

/**
 * The single task of running the command where pnpm was invoked. Selects the
 * project at `opts.dir` when it has a manifest.
 */
async function buildSingleProjectExecTaskGraph (opts: ExecOpts, commandName: string): Promise<TaskGraph> {
  const project = (opts.cliOptions.dir ?? process.cwd()) as ProjectRootDir
  const taskGraph: TaskGraph = new Map([[taskKey(project, commandName), {
    project,
    taskName: commandName,
    scripts: [commandName],
    requested: true,
    dependencies: [],
  }]])
  const projectManifest = await tryReadProjectManifest(opts.dir)
  if (projectManifest.manifest != null) {
    opts.selectedProjectsGraph = {
      [opts.dir]: {
        dependencies: [],
        package: {
          ...projectManifest,
          rootDir: opts.dir as ProjectRootDir,
          rootDirRealPath: opts.dir as ProjectRootDirRealPath,
        } as Project,
      },
    }
  }
  return taskGraph
}

function createExecTaskRunStateContext (
  { opts, params, baseExtraEnv, fullTaskGraph }: {
    opts: ExecOpts
    params: string[]
    baseExtraEnv: Record<string, string | undefined>
    fullTaskGraph: TaskGraph
  }
): TaskRunStateContext {
  return new TaskRunStateContext({
    command: 'exec',
    params: [...params, `shell-mode=${Boolean(opts.shellMode)}`],
    settings: taskRunExecutionSettings({ ...opts, extraEnv: baseExtraEnv }),
    graph: fullTaskGraph,
    workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
    scriptCommands: () => [],
  })
}

async function resumeExecTaskGraph (
  fullTaskGraph: TaskGraph,
  { opts, commandName, taskRunStateContext }: { opts: ExecOpts, commandName: string, taskRunStateContext: TaskRunStateContext | undefined }
): Promise<TaskGraph> {
  if (!opts.resumeFrom) return fullTaskGraph
  const resumeOptions = {
    resumeFrom: opts.resumeFrom,
    selectedProjectsGraph: opts.selectedProjectsGraph,
    taskName: commandName,
  }
  const taskGraph = resumeTaskGraphFrom(fullTaskGraph, resumeOptions)
  const completedTasks = await taskRunStateContext?.readCompletedTasks()
  if (completedTasks == null) return taskGraph
  return resumeTaskGraphFrom(fullTaskGraph, { ...resumeOptions, completedTasks })
}

function collectTasksOutsideGraph (fullTaskGraph: TaskGraph, taskGraph: TaskGraph): Set<TaskKey> {
  const tasksOutsideGraph = new Set<TaskKey>()
  for (const key of fullTaskGraph.keys()) {
    if (!taskGraph.has(key)) tasksOutsideGraph.add(key)
  }
  return tasksOutsideGraph
}
