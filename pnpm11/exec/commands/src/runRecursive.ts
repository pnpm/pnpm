import assert from 'node:assert'
import path from 'node:path'
import util from 'node:util'

import { type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, getWorkspaceConcurrency } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  type RunLifecycleHookOptions,
} from '@pnpm/exec.lifecycle'
import { groupStart } from '@pnpm/log.group'
import { globalWarn } from '@pnpm/logger'
import type { PackageScripts, ProjectRootDir } from '@pnpm/types'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import {
  buildTaskGraph,
  isSerialTaskGraph,
  renderTaskGraphDryRun,
  resumeTaskGraphFrom,
  reverseTaskGraph,
  scheduleTasks,
  sequenceTasks,
  type TaskCompletion,
  type TaskGraph,
  taskGraphToJson,
  type TaskKey,
  type TaskNode,
} from '@pnpm/workspace.task-scheduler'
import pLimit from 'p-limit'
import { realpathMissing } from 'realpath-missing'

import { getExecutionDuration, writeRecursiveSummary } from './exec.js'
import { existsInDir } from './existsInDir.js'
import { throwOrFilterHiddenScripts } from './hiddenScripts.js'
import { tryBuildRegExpFromCommand } from './regexpCommand.js'
import { getRunScriptCommands, runScript, type RunScriptOptions } from './run.js'
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
Partial<Pick<Config, 'extraBinPaths' | 'extraEnv' | 'bail' | 'dryRun' | 'ignoreWorkspaceCycles' | 'reporter' | 'reverse' | 'sort' | 'tasks' | 'workspaceConcurrency'>> &
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

  const fullTaskGraph = buildRunTaskGraph(scriptName, opts)
  const taskRunStateContext = new TaskRunStateContext({
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
  let taskGraph = fullTaskGraph
  if (opts.resumeFrom != null) {
    const resumeOptions = {
      resumeFrom: opts.resumeFrom,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      taskName: scriptName,
    }
    taskGraph = resumeTaskGraphFrom(fullTaskGraph, resumeOptions)
    const completedTasks = await taskRunStateContext.readCompletedTasks()
    if (completedTasks != null) {
      taskGraph = resumeTaskGraphFrom(fullTaskGraph, { ...resumeOptions, completedTasks })
    }
  }
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

  const requiredScripts = opts.requiredScripts ?? []
  if (requiredScripts.includes(scriptName)) {
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

  if (!process.env.npm_lifecycle_event) {
    for (const node of taskGraph.values()) {
      if (!node.requested) continue
      node.scripts = throwOrFilterHiddenScripts(node.scripts, scriptName)
    }
  }

  // Before anything is dispatched: when no selected project has the script,
  // the run is a user error, and the tasks `dependsOn` pulled in must not
  // have run their side effects by the time it is reported.
  if (scriptName !== 'test' && !opts.ifPresent && [...taskGraph.values()].every((node) => !node.requested || node.scripts.length === 0)) {
    throw noRequestedScriptError(scriptName, opts)
  }

  const initiallyCompleted = new Set<TaskKey>()
  for (const key of fullTaskGraph.keys()) {
    if (!taskGraph.has(key)) initiallyCompleted.add(key)
  }
  const taskRunState = await taskRunStateContext.start(initiallyCompleted)

  const limitRun = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency))
  const stdio =
    !opts.stream &&
    (opts.workspaceConcurrency === 1 || isSerialTaskGraph(taskGraph, sequencedTasks))
      ? 'inherit'
      : 'pipe'
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && existsPnp(opts.workspaceDir)
  const existsPackageMap = existsInDir.bind(null, path.join(opts.modulesDir ?? 'node_modules', '.package-map.json'))
  const workspacePackageMapPath = opts.nodeExperimentalPackageMap && opts.workspaceDir && existsPackageMap(opts.workspaceDir)

  const result: RecursiveSummary = {}
  for (const node of taskGraph.values()) {
    result[taskSummaryKey(node)] = { status: 'queued' }
  }
  let hasCommand = 0
  let firstError: Error | undefined
  let abortError: unknown

  const runTask = async (node: TaskNode, key: TaskKey): Promise<TaskCompletion> => {
    try {
      return await runTaskScripts(node, key)
    } catch (err: unknown) {
      // An error the per-script handling could not absorb is an
      // infrastructure failure: hold it for rethrow and stop the run.
      abortError ??= err
      return 'aborted'
    }
  }

  const runTaskScripts = async (node: TaskNode, key: TaskKey): Promise<TaskCompletion> => {
    const pkg = opts.selectedProjectsGraph[node.project]
    const summaryKey = taskSummaryKey(node)
    // A RegExp selector can match several scripts in one task, but the
    // summary carries a single status per task and countFailures derives
    // the exit code from it. Once one of a task's scripts has failed,
    // nothing a later one does may overwrite that — under --no-bail the run
    // would otherwise report itself green. Tracked in a local because the
    // scripts settle concurrently, so reading back the recorded status
    // would race (and TypeScript narrows it to 'running' regardless).
    let taskFailed = false
    let taskCancelled = false
    let taskSkippedForCurrentLifecycle = false
    await Promise.all(node.scripts.map(async (script) =>
      limitRun(async () => {
        // Under --bail a failure stops dispatch, but a script already queued
        // behind the concurrency limit has been dispatched in name only —
        // starting it now would grow the failed run. It stays 'queued'.
        if (opts.bail && firstError != null) {
          taskCancelled = true
          return
        }
        if (!pkg.package.manifest.scripts?.[script]) {
          return
        }
        if (
          process.env.npm_lifecycle_event === script &&
          process.env.PNPM_SCRIPT_SRC_DIR === node.project
        ) {
          taskSkippedForCurrentLifecycle = true
          return
        }
        if (!taskFailed) {
          result[summaryKey].status = 'running'
        }
        const startTime = process.hrtime()
        if (node.requested) {
          hasCommand++
        }
        try {
          const lifecycleOpts: RunLifecycleHookOptions = {
            depPath: node.project,
            extraBinPaths: opts.extraBinPaths,
            extraEnv: opts.extraEnv,
            pkgRoot: node.project,
            userAgent: opts.userAgent,
            rootModulesDir: await realpathMissing(path.join(node.project, 'node_modules')),
            scriptsPrependNodePath: opts.scriptsPrependNodePath,
            scriptShell: opts.scriptShell,
            silent: opts.reporter === 'silent',
            shellEmulator: opts.shellEmulator,
            stdio,
            unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
          }
          const pnpPath = workspacePnpPath ?? existsPnp(node.project)
          if (pnpPath) {
            lifecycleOpts.extraEnv = {
              ...lifecycleOpts.extraEnv,
              ...makeNodeRequireOption(pnpPath, lifecycleOpts.extraEnv),
            }
          }
          const packageMapPath = workspacePackageMapPath || (opts.nodeExperimentalPackageMap && existsPackageMap(node.project))
          if (packageMapPath) {
            lifecycleOpts.extraEnv = {
              ...lifecycleOpts.extraEnv,
              ...makeNodePackageMapOption(packageMapPath, lifecycleOpts.extraEnv),
            }
          }

          const runScriptOptions: RunScriptOptions = {
            enablePrePostScripts: opts.enablePrePostScripts ?? false,
            syncInjectedDepsAfterScripts: opts.syncInjectedDepsAfterScripts,
            workspaceDir: opts.workspaceDir,
          }
          const _runScript = runScript.bind(null, { manifest: pkg.package.manifest, lifecycleOpts, runScriptOptions, passedThruArgs })
          const groupEnd = Boolean(lifecycleOpts.silent) || getWorkspaceConcurrency(opts.workspaceConcurrency) > 1
            ? undefined
            : groupStart(formatSectionName({
              name: pkg.package.manifest.name,
              script,
              version: pkg.package.manifest.version,
              prefix: path.normalize(path.relative(opts.workspaceDir, node.project)),
            }))
          await _runScript(script)
          groupEnd?.()
          if (!taskFailed) {
            result[summaryKey].status = 'passed'
            result[summaryKey].duration = getExecutionDuration(startTime)
          }
        } catch (err: unknown) {
          assert(util.types.isNativeError(err))
          taskFailed = true
          result[summaryKey] = {
            status: 'failure',
            duration: getExecutionDuration(startTime),
            error: err,
            message: err.message,
            prefix: node.project,
          }
          if (opts.bail && firstError == null) {
            Object.assign(err, {
              code: 'ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL',
              prefix: node.project,
            })
            firstError = err
          }
        }
      })))
    if (taskFailed || taskCancelled) return 'failed'
    if (!taskSkippedForCurrentLifecycle) {
      await taskRunState.recordPassed(key, node)
    }
    return 'passed'
  }

  try {
    await scheduleTasks(taskGraph, {
      bail: Boolean(opts.bail),
      runTask,
      onTaskSkipped: (node) => {
        result[taskSummaryKey(node)].status = 'skipped'
      },
    })

    if (abortError !== undefined) {
      throw abortError
    }
    if (firstError != null) {
      if (opts.reportSummary) {
        await writeRecursiveSummary({
          dir: opts.workspaceDir ?? opts.dir,
          summary: result,
        })
      }
      throw firstError
    }

    // The no-script error is only for a run that had nothing to do. A run
    // where a `dependsOn`-pulled task failed and skipped every requested task
    // must report that failure, not claim the script does not exist.
    const hasFailures = Object.values(result).some(({ status }) => status === 'failure')
    if (scriptName !== 'test' && !hasCommand && !hasFailures && !opts.ifPresent) {
      await taskRunState.finish()
      throw noRequestedScriptError(scriptName, opts)
    }
    if (opts.reportSummary) {
      await writeRecursiveSummary({
        dir: opts.workspaceDir ?? opts.dir,
        summary: result,
      })
    }
    throwOnCommandFail('pnpm recursive run', result)
    await taskRunState.finish()
    return undefined
  } catch (err: unknown) {
    await taskRunState.close()
    throw err
  }
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
    tasks: opts.sort ? opts.tasks : undefined,
  })
  if (opts.reverse) {
    taskGraph = reverseTaskGraph(taskGraph)
  }
  return taskGraph
}

/**
 * The task's key in the recursive summary. The task of the script the
 * invocation named keeps the project directory alone — the format existing
 * consumers of `pnpm-exec-summary.json` read — and only tasks `dependsOn`
 * pulled in qualify it with the task name.
 */
function noRequestedScriptError (scriptName: string, opts: RecursiveRunOpts): PnpmError {
  const allPackagesAreSelected = Object.keys(opts.selectedProjectsGraph).length === opts.allProjects.length
  return allPackagesAreSelected
    ? new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the packages has a "${scriptName}" script`)
    : new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the selected packages has a "${scriptName}" script`)
}

function taskSummaryKey (node: TaskNode): string {
  return node.requested ? node.project : `${node.project}#${node.taskName}`
}

function formatSectionName ({
  script,
  name,
  version,
  prefix,
}: {
  script?: string
  name?: string
  version?: string
  prefix: string
}) {
  return `${name ?? 'unknown'}${version ? `@${version}` : ''} ${script ? `: ${script}` : ''} ${prefix}`
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
