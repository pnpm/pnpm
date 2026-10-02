import assert from 'node:assert'
import path from 'node:path'

import { type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli.utils'
import { binDirOf, getWorkspaceConcurrency } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import {
  makeNodePackageMapOption,
  makeNodeRequireOption,
  makeProjectNodePathOption,
  type RunLifecycleHookOptions,
} from '@pnpm/exec.lifecycle'
import { groupStart } from '@pnpm/log.group'
import type { ProjectManifest } from '@pnpm/types'
import {
  isSerialTaskGraph,
  scheduleTasks,
  type TaskCompletion,
  type TaskGraph,
  type TaskKey,
  type TaskNode,
} from '@pnpm/workspace.task-scheduler'
import pLimit, { type LimitFunction } from 'p-limit'
import { realpathMissing } from 'realpath-missing'

import { suppressesScriptEcho } from './createLifecycleOpts.js'
import { existsInDir } from './existsInDir.js'
import { getExecutionDuration, writeRecursiveSummary } from './recursiveSummary.js'
import type { RecursiveRunOpts } from './runRecursive.js'
import { runScript, type RunScriptOptions } from './runScript.js'
import type { TaskRunState } from './taskRunState.js'

export interface RunTaskGraphOptions {
  opts: RecursiveRunOpts
  scriptName: string
  passedThruArgs: string[]
  taskGraph: TaskGraph
  sequencedTasks: TaskKey[]
  taskRunState: TaskRunState
  modulesDirFor: (projectName: string | undefined) => string | undefined
}

interface RecursiveRunContext {
  opts: RecursiveRunOpts
  scriptName: string
  passedThruArgs: string[]
  taskRunState: TaskRunState
  modulesDirFor: (projectName: string | undefined) => string | undefined
  limitRun: LimitFunction
  stdio: 'inherit' | 'pipe'
  existsPnp: (dir: string) => string | undefined
  workspacePnpPath: string | undefined
  existsPackageMap: (dir: string) => string | undefined
  workspacePackageMapPath: string | false | undefined
  result: RecursiveSummary
  hasCommand: number
  firstError: Error | undefined
  abortError: unknown
}

/**
 * The outcome of one task's scripts. A RegExp selector can match several
 * scripts in one task, but the summary carries a single status per task and
 * countFailures derives the exit code from it. Once one of a task's scripts
 * has failed, nothing a later one does may overwrite that — under --no-bail
 * the run would otherwise report itself green. Tracked apart from the summary
 * because the scripts settle concurrently, so reading back the recorded status
 * would race.
 */
interface TaskOutcome {
  failed: boolean
  cancelled: boolean
  skippedForCurrentLifecycle: boolean
}

export async function runTaskGraph (runOpts: RunTaskGraphOptions): Promise<undefined> {
  const ctx = createRecursiveRunContext(runOpts)
  try {
    await scheduleAndReport(ctx, runOpts.taskGraph)
    return undefined
  } catch (err: unknown) {
    await ctx.taskRunState.close()
    throw err
  }
}

function createRecursiveRunContext (runOpts: RunTaskGraphOptions): RecursiveRunContext {
  const { opts, taskGraph, sequencedTasks } = runOpts
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
    result[taskSummaryKey(node, runOpts.scriptName)] = { status: 'queued' }
  }
  return {
    opts,
    scriptName: runOpts.scriptName,
    passedThruArgs: runOpts.passedThruArgs,
    taskRunState: runOpts.taskRunState,
    modulesDirFor: runOpts.modulesDirFor,
    limitRun,
    stdio,
    existsPnp,
    workspacePnpPath,
    existsPackageMap,
    workspacePackageMapPath,
    result,
    hasCommand: 0,
    firstError: undefined,
    abortError: undefined,
  }
}

async function scheduleAndReport (ctx: RecursiveRunContext, taskGraph: TaskGraph): Promise<void> {
  const { opts, scriptName, result } = ctx
  await scheduleTasks(taskGraph, {
    bail: Boolean(opts.bail),
    runTask: async (node, key) => runTask(ctx, node, key),
    onTaskSkipped: (node) => {
      result[taskSummaryKey(node, scriptName)].status = 'skipped'
    },
  })

  if (ctx.abortError !== undefined) {
    throw ctx.abortError
  }
  if (ctx.firstError != null) {
    if (opts.reportSummary) {
      await writeRecursiveSummary({
        dir: opts.workspaceDir ?? opts.dir,
        summary: result,
      })
    }
    throw ctx.firstError
  }

  // The no-script error is only for a run that had nothing to do. A run
  // where a `dependsOn`-pulled task failed and skipped every requested task
  // must report that failure, not claim the script does not exist.
  const hasFailures = Object.values(result).some(({ status }) => status === 'failure')
  if (scriptName !== 'test' && !ctx.hasCommand && !hasFailures && !opts.ifPresent) {
    await ctx.taskRunState.finish()
    throw noRequestedScriptError(scriptName, opts)
  }
  if (opts.reportSummary) {
    await writeRecursiveSummary({
      dir: opts.workspaceDir ?? opts.dir,
      summary: result,
    })
  }
  throwOnCommandFail('pnpm recursive run', result)
  await ctx.taskRunState.finish()
}

async function runTask (ctx: RecursiveRunContext, node: TaskNode, key: TaskKey): Promise<TaskCompletion> {
  try {
    return await runTaskScripts(ctx, node, key)
  } catch (err: unknown) {
    // An error the per-script handling could not absorb is an
    // infrastructure failure: hold it for rethrow and stop the run.
    ctx.abortError ??= err
    return 'aborted'
  }
}

async function runTaskScripts (ctx: RecursiveRunContext, node: TaskNode, key: TaskKey): Promise<TaskCompletion> {
  const outcome: TaskOutcome = { failed: false, cancelled: false, skippedForCurrentLifecycle: false }
  await Promise.all(node.scripts.map(async (script) =>
    ctx.limitRun(async () => runTaskScript(ctx, { node, script, outcome }))))
  if (outcome.failed || outcome.cancelled) return 'failed'
  if (!outcome.skippedForCurrentLifecycle) {
    await ctx.taskRunState.recordPassed(key, node)
  }
  return 'passed'
}

interface TaskScript {
  node: TaskNode
  script: string
  outcome: TaskOutcome
}

async function runTaskScript (ctx: RecursiveRunContext, { node, script, outcome }: TaskScript): Promise<void> {
  // Under --bail a failure stops dispatch, but a script already queued
  // behind the concurrency limit has been dispatched in name only —
  // starting it now would grow the failed run. It stays 'queued'.
  if (ctx.opts.bail && ctx.firstError != null) {
    outcome.cancelled = true
    return
  }
  const manifest = ctx.opts.selectedProjectsGraph[node.project].package.manifest
  if (!manifest.scripts?.[script]) {
    return
  }
  if (isScriptOfCurrentLifecycle(node, script)) {
    outcome.skippedForCurrentLifecycle = true
    return
  }
  const summaryKey = taskSummaryKey(node, ctx.scriptName)
  if (!outcome.failed) {
    ctx.result[summaryKey].status = 'running'
  }
  const startTime = process.hrtime()
  if (node.requested) {
    ctx.hasCommand++
  }
  try {
    await runProjectScript(ctx, { node, script, manifest })
    recordScriptPassed(ctx, { summaryKey, startTime, outcome })
  } catch (err: unknown) {
    recordScriptFailure(ctx, { node, summaryKey, startTime, outcome, err })
  }
}

function isScriptOfCurrentLifecycle (node: TaskNode, script: string): boolean {
  return process.env.npm_lifecycle_event === script &&
    process.env.PNPM_SCRIPT_SRC_DIR === node.project
}

async function runProjectScript (
  ctx: RecursiveRunContext,
  { node, script, manifest }: { node: TaskNode, script: string, manifest: ProjectManifest }
): Promise<void> {
  const { opts } = ctx
  const lifecycleOpts = await createTaskLifecycleOpts(ctx, node, manifest)
  const runScriptOptions: RunScriptOptions = {
    enablePrePostScripts: opts.enablePrePostScripts ?? false,
    syncInjectedDepsAfterScripts: opts.syncInjectedDepsAfterScripts,
    workspaceDir: opts.workspaceDir,
  }
  const _runScript = runScript.bind(null, { manifest, lifecycleOpts, runScriptOptions, passedThruArgs: ctx.passedThruArgs })
  const groupEnd = Boolean(lifecycleOpts.silent) || getWorkspaceConcurrency(opts.workspaceConcurrency) > 1
    ? undefined
    : groupStart(formatSectionName({
      name: manifest.name,
      script,
      version: manifest.version,
      prefix: path.normalize(path.relative(opts.workspaceDir, node.project)),
    }))
  try {
    await _runScript(script)
  } finally {
    groupEnd?.()
  }
}

async function createTaskLifecycleOpts (
  ctx: RecursiveRunContext,
  node: TaskNode,
  manifest: ProjectManifest
): Promise<RunLifecycleHookOptions> {
  const { opts } = ctx
  const wdBinDir = binDirOf(node.project, ctx.modulesDirFor(manifest.name))
  const lifecycleOpts: RunLifecycleHookOptions = {
    depPath: node.project,
    wdBinDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: { ...opts.extraEnv, ...await makeProjectNodePathOption({ modulesDir: path.dirname(wdBinDir), rootDir: node.project }, opts) },
    pkgRoot: node.project,
    raiseOnInterrupt: true,
    userAgent: opts.userAgent,
    rootModulesDir: await realpathMissing(path.dirname(wdBinDir)),
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    silent: suppressesScriptEcho(opts),
    shellEmulator: opts.shellEmulator,
    stdio: ctx.stdio,
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  }
  const pnpPath = ctx.workspacePnpPath ?? ctx.existsPnp(node.project)
  if (pnpPath) {
    lifecycleOpts.extraEnv = {
      ...lifecycleOpts.extraEnv,
      ...makeNodeRequireOption(pnpPath, lifecycleOpts.extraEnv),
    }
  }
  const packageMapPath = ctx.workspacePackageMapPath || (opts.nodeExperimentalPackageMap && ctx.existsPackageMap(node.project))
  if (packageMapPath) {
    lifecycleOpts.extraEnv = {
      ...lifecycleOpts.extraEnv,
      ...makeNodePackageMapOption(packageMapPath, lifecycleOpts.extraEnv),
    }
  }
  return lifecycleOpts
}

function recordScriptPassed (
  ctx: RecursiveRunContext,
  { summaryKey, startTime, outcome }: { summaryKey: string, startTime: [number, number], outcome: TaskOutcome }
): void {
  if (outcome.failed) return
  ctx.result[summaryKey].status = 'passed'
  ctx.result[summaryKey].duration = getExecutionDuration(startTime)
}

interface ScriptFailure {
  node: TaskNode
  summaryKey: string
  startTime: [number, number]
  outcome: TaskOutcome
  err: unknown
}

function recordScriptFailure (ctx: RecursiveRunContext, { node, summaryKey, startTime, outcome, err }: ScriptFailure): void {
  assert(isError(err))
  outcome.failed = true
  ctx.result[summaryKey] = {
    status: 'failure',
    duration: getExecutionDuration(startTime),
    error: err,
    message: err.message,
    prefix: node.project,
  }
  if (ctx.opts.bail && ctx.firstError == null) {
    Object.assign(err, {
      code: 'ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL',
      prefix: node.project,
    })
    ctx.firstError = err
  }
}

export function noRequestedScriptError (scriptName: string, opts: RecursiveRunOpts): PnpmError {
  const allPackagesAreSelected = Object.keys(opts.selectedProjectsGraph).length === opts.allProjects.length
  return allPackagesAreSelected
    ? new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the packages has a "${scriptName}" script`)
    : new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the selected packages has a "${scriptName}" script`)
}

/**
 * The task's key in the recursive summary. The task of the script the
 * invocation named keeps the project directory alone, the format existing
 * consumers of `pnpm-exec-summary.json` read. Every other task qualifies it with the task name: those `dependsOn`
 * pulled in, and the per-script tasks a RegExp selector expands into.
 */
function taskSummaryKey (node: TaskNode, scriptName: string): string {
  return node.requested && node.taskName === scriptName ? node.project : `${node.project}#${node.taskName}`
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
