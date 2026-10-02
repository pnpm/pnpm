import path from 'node:path'
import { StringDecoder } from 'node:string_decoder'

import { readProjectManifestOnly, type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli.utils'
import { binDirOf } from '@pnpm/config.reader'
import { lifecycleLogger, type LifecycleMessage } from '@pnpm/core-loggers'
import { makeNodePackageMapOption, makeNodeRequireOption, makeProjectNodePathOption } from '@pnpm/exec.lifecycle'
import { logger } from '@pnpm/logger'
import type { ProjectRootDir } from '@pnpm/types'
import {
  scheduleTasks,
  type TaskCompletion,
  type TaskGraph,
  type TaskKey,
  type TaskNode,
} from '@pnpm/workspace.task-scheduler'
import type { LimitFunction } from 'p-limit'

import type { ExecOpts } from './exec.js'
import { type CommandError, createExecCommandNotFoundHint, isErrorCommandNotFound } from './execCommandNotFound.js'
import { existsInDir } from './existsInDir.js'
import { makeEnv } from './makeEnv.js'
import { getExecutionDuration, writeRecursiveSummary } from './recursiveSummary.js'
import type { TaskRunState } from './taskRunState.js'
import { signalReaching, trackedExeca, waitForTracked } from './trackedExeca.js'

type TrackedChild = ReturnType<typeof trackedExeca>

export interface RunExecTaskGraphOptions {
  opts: ExecOpts
  params: string[]
  taskGraph: TaskGraph
  taskRunState: TaskRunState | undefined
  baseExtraEnv: Record<string, string | undefined>
  modulesDirFor: (projectName: string | undefined) => string | undefined
  limitRun: LimitFunction
}

interface ExecRunContext extends Omit<RunExecTaskGraphOptions, 'taskGraph'> {
  result: RecursiveSummary
  existsPnp: (dir: string) => string | undefined
  workspacePnpPath: string | undefined
  existsPackageMap: (dir: string) => string | undefined
  workspacePackageMapPath: string | false | undefined
  exitCode: number
  firstError: Error | undefined
  abortError: unknown
  interruptedBy: NodeJS.Signals | null
  /**
   * Every command's wait, so that after a signal the run ends only once all
   * of them have settled: the signal pnpm then raises on itself would
   * otherwise reach the relays of commands still shutting down.
   */
  settling: Array<Promise<unknown>>
  reporterShowPrefix: boolean
}

interface ProjectCommand {
  prefix: string
  projectDir: ProjectRootDir
  modulesDir: string | undefined
  prependPaths: string[]
}

type ExecCommandError = CommandError & {
  hint?: string
  exitCode?: unknown
  code?: string
  prefix?: string
}

export async function runExecTaskGraph (runOpts: RunExecTaskGraphOptions): Promise<{ exitCode: number }> {
  const ctx = createExecRunContext(runOpts)
  try {
    await scheduleAndReport(ctx, runOpts.taskGraph)
    return { exitCode: ctx.exitCode }
  } catch (err: unknown) {
    await ctx.taskRunState?.close()
    throw err
  }
}

function createExecRunContext (runOpts: RunExecTaskGraphOptions): ExecRunContext {
  const { opts, taskGraph } = runOpts
  const result: RecursiveSummary = {}
  for (const node of taskGraph.values()) {
    result[node.project] = { status: 'queued' }
  }
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && existsPnp(opts.workspaceDir)
  const existsPackageMap = existsInDir.bind(null, path.join(opts.modulesDir ?? 'node_modules', '.package-map.json'))
  const workspacePackageMapPath = opts.nodeExperimentalPackageMap && opts.workspaceDir && existsPackageMap(opts.workspaceDir)
  return {
    opts,
    params: runOpts.params,
    taskRunState: runOpts.taskRunState,
    baseExtraEnv: runOpts.baseExtraEnv,
    modulesDirFor: runOpts.modulesDirFor,
    limitRun: runOpts.limitRun,
    result,
    existsPnp,
    workspacePnpPath,
    existsPackageMap,
    workspacePackageMapPath,
    exitCode: 0,
    firstError: undefined,
    abortError: undefined,
    interruptedBy: null,
    settling: [],
    reporterShowPrefix: Boolean(opts.recursive) && opts.reporterHidePrefix === false,
  }
}

async function scheduleAndReport (ctx: ExecRunContext, taskGraph: TaskGraph): Promise<void> {
  const { opts, result } = ctx
  await scheduleTasks(taskGraph, {
    bail: Boolean(opts.bail),
    runTask: async (node, key) => runTask(ctx, node, key),
    onTaskSkipped: (node) => {
      result[node.project].status = 'skipped'
    },
  })

  if (ctx.abortError !== undefined) {
    throw ctx.abortError
  }
  if (ctx.interruptedBy && opts.recursive) {
    await raiseInterruptingSignal(ctx, ctx.interruptedBy)
  }
  if (ctx.firstError != null) {
    if (opts.reportSummary) {
      await writeRecursiveSummary({
        dir: opts.lockfileDir ?? opts.dir,
        summary: result,
      })
    }
    throw ctx.firstError
  }

  if (opts.reportSummary) {
    await writeRecursiveSummary({
      dir: opts.lockfileDir ?? opts.dir,
      summary: result,
    })
  }
  throwOnCommandFail('pnpm recursive exec', result)
  await ctx.taskRunState?.finish()
}

/**
 * A single command's exit status is pnpm's, however it ended. A recursive
 * run that a signal cut short ends the way the signal would have ended pnpm,
 * so the shell sees an interrupted run rather than the status of whichever
 * command finished last. The signal arrives through the event loop, so pnpm
 * waits for it rather than racing it to its own exit.
 */
async function raiseInterruptingSignal (ctx: ExecRunContext, signal: NodeJS.Signals): Promise<void> {
  await Promise.allSettled(ctx.settling)
  process.kill(process.pid, signal)
  await new Promise<void>((resolve) => setTimeout(resolve, 1000))
}

async function runTask (ctx: ExecRunContext, node: TaskNode, key: TaskKey): Promise<TaskCompletion> {
  try {
    return await ctx.limitRun(async () => runCommandTask(ctx, node, key))
  } catch (err: unknown) {
    // An error the per-project handling could not absorb is an
    // infrastructure failure: hold it for rethrow and stop the run.
    ctx.abortError ??= err
    return 'aborted'
  }
}

async function runCommandTask (ctx: ExecRunContext, node: TaskNode, key: TaskKey): Promise<TaskCompletion> {
  // Under --bail a failure stops dispatch, and so does a signal that
  // reached pnpm, but a task already queued behind the concurrency
  // limit has been dispatched in name only — starting it now would
  // grow the failed or interrupted run. It stays 'queued'.
  if ((ctx.opts.bail && ctx.firstError != null) || ctx.interruptedBy) {
    return 'passed'
  }
  const command = resolveProjectCommand(ctx, node)
  const { prefix } = command
  ctx.result[prefix].status = 'running'
  const startTime = process.hrtime()
  const spawned: { child?: TrackedChild } = {}
  try {
    const env = await createCommandEnv(ctx, command)
    await spawnCommand(ctx, { prefix, env, spawned })
    ctx.result[prefix].status = 'passed'
    ctx.result[prefix].duration = getExecutionDuration(startTime)
  } catch (err: unknown) {
    return handleCommandFailure(ctx, { err: err as ExecCommandError, command, startTime, child: spawned.child })
  }
  // A signal that reached pnpm ends the run once the commands in
  // flight have finished; nothing queued behind them starts, and a
  // command the signal cut short is not journaled as passed, so a
  // resumed run repeats it.
  if (ctx.interruptedBy) {
    return 'aborted'
  }
  await ctx.taskRunState?.recordPassed(key, node)
  return 'passed'
}

function resolveProjectCommand (ctx: ExecRunContext, node: TaskNode): ProjectCommand {
  const { opts } = ctx
  const prefix = node.project
  // Without --recursive the command runs where pnpm was invoked, which
  // may be a plain subdirectory of the project at `opts.dir`. The
  // project's bin directory is added relative to the run directory, like
  // `./node_modules/.bin`, so a project path that contains the PATH
  // delimiter stays out of PATH.
  const projectDir = opts.recursive ? prefix : opts.dir as ProjectRootDir
  const modulesDir = ctx.modulesDirFor(opts.selectedProjectsGraph[projectDir]?.package.manifest.name)
  const prependPaths = [
    modulesDir ? path.relative(prefix, binDirOf(prefix, modulesDir)) : './node_modules/.bin',
    ...(projectDir !== prefix ? [path.relative(prefix, binDirOf(projectDir, modulesDir))] : []),
    ...(opts.extraBinPaths ?? []),
  ]
  return { prefix, projectDir, modulesDir, prependPaths }
}

async function createCommandEnv (ctx: ExecRunContext, command: ProjectCommand): Promise<NodeJS.ProcessEnv> {
  const { opts, baseExtraEnv } = ctx
  const { prefix, projectDir, modulesDir } = command
  const pnpPath = ctx.workspacePnpPath ?? ctx.existsPnp(projectDir)
  const packageMapPath = ctx.workspacePackageMapPath || (opts.nodeExperimentalPackageMap && ctx.existsPackageMap(projectDir))
  const extraEnv = {
    ...baseExtraEnv,
    ...await makeProjectNodePathOption(
      { modulesDir: path.dirname(binDirOf(projectDir, modulesDir)), rootDir: projectDir },
      { ...opts, extraEnv: baseExtraEnv }
    ),
  }
  if (pnpPath) {
    Object.assign(extraEnv, makeNodeRequireOption(pnpPath, extraEnv))
  }
  if (packageMapPath) {
    Object.assign(extraEnv, makeNodePackageMapOption(packageMapPath, extraEnv))
  }
  return makeEnv({
    extraEnv: {
      ...extraEnv,
      PNPM_PACKAGE_NAME: opts.selectedProjectsGraph[projectDir]?.package.manifest.name,
      // The child inherits the PWD of pnpm's own cwd. When the command
      // runs in that same directory the inherited value is already
      // right and may hold the logical path through a symlink, so keep
      // it. Otherwise point PWD at the command's cwd: shells trust PWD
      // over the physical working directory, so a project reached
      // through a symlink then reports its logical path. Skipped on
      // Windows, where PWD is a POSIX convention that neither cmd.exe
      // nor PowerShell reads.
      ...(process.platform !== 'win32' && prefix !== process.cwd() ? { PWD: prefix } : {}),
    },
    prependPaths: command.prependPaths,
    userAgent: opts.userAgent,
  })
}

interface CommandSpawn {
  prefix: string
  env: NodeJS.ProcessEnv
  /** Receives the child as soon as it is spawned, so a failure can still tell whether a signal reached it. */
  spawned: { child?: TrackedChild }
}

async function spawnCommand (ctx: ExecRunContext, spawn: CommandSpawn): Promise<void> {
  if (ctx.reporterShowPrefix) {
    await spawnCommandWithPrefixedOutput(ctx, spawn)
    return
  }
  const [cmd, ...args] = ctx.params
  const child = trackedExeca(cmd, args, {
    cwd: spawn.prefix,
    env: spawn.env,
    stdio: 'inherit',
    shell: ctx.opts.shellMode ?? false,
  })
  spawn.spawned.child = child
  const settled = waitForTracked(child)
  ctx.settling.push(settled)
  const signal = await settled
  ctx.interruptedBy ??= signal
}

async function spawnCommandWithPrefixedOutput (ctx: ExecRunContext, { prefix, env, spawned }: CommandSpawn): Promise<void> {
  const [cmd, ...args] = ctx.params
  const manifest = await readProjectManifestOnly(prefix)
  const child = trackedExeca(cmd, args, {
    cwd: prefix,
    env,
    stdio: 'pipe',
    shell: ctx.opts.shellMode ?? false,
  })
  spawned.child = child
  // Registered before the output is drained, so a signal that ends
  // the run waits for this command however far its output is.
  const settled = waitForTracked(child)
  ctx.settling.push(settled)
  const lifecycleOpts = {
    wd: prefix,
    depPath: manifest.name ?? path.relative(ctx.opts.dir, prefix),
    stage: '(exec)',
  } satisfies Partial<LifecycleMessage>
  const stdoutLog = createOutputLineLogger(lifecycleOpts, 'stdout')
  const stderrLog = createOutputLineLogger(lifecycleOpts, 'stderr')
  child.stdout!.on('data', stdoutLog.onData)
  child.stderr!.on('data', stderrLog.onData)
  await new Promise<void>((resolve) => {
    void child.once('close', exitCode => {
      stdoutLog.onEnd()
      stderrLog.onEnd()
      lifecycleLogger.debug({
        ...lifecycleOpts,
        exitCode: exitCode ?? 1,
        optional: false,
      })
      resolve()
    })
  })
  const signal = await settled
  ctx.interruptedBy ??= signal
}

interface OutputLineLogger {
  onData: (data: Buffer | string) => void
  onEnd: () => void
}

/**
 * A chunk is neither a line nor a whole number of characters: it may end
 * mid-line, mid-character, or on a newline (which would otherwise report a
 * trailing empty line). A StringDecoder holds back the bytes of a split
 * character, and `pending` holds back a partial line; both are flushed when
 * the stream ends. Only the newly arrived text is scanned for newlines, so a
 * long line costs one concatenation rather than a re-split of everything held.
 */
function createOutputLineLogger (
  lifecycleOpts: { wd: string, depPath: string, stage: string },
  stdio: 'stdout' | 'stderr'
): OutputLineLogger {
  const log = (line: string): void => {
    lifecycleLogger.debug({ ...lifecycleOpts, stdio, line })
  }
  const decoder = new StringDecoder('utf8')
  let pending = ''
  return {
    onData (data: Buffer | string): void {
      pending = logCompleteLines(pending, typeof data === 'string' ? data : decoder.write(data), log)
    },
    onEnd (): void {
      pending = logCompleteLines(pending, decoder.end(), log)
      if (pending !== '') {
        log(pending)
        pending = ''
      }
    },
  }
}

/** Logs every line that `text` completes and returns the partial line left over. */
function logCompleteLines (pending: string, text: string, log: (line: string) => void): string {
  let held = pending
  let start = 0
  for (let end = text.indexOf('\n'); end !== -1; end = text.indexOf('\n', start)) {
    const line = held + text.slice(start, end)
    held = ''
    start = end + 1
    // A CRLF terminator contributes no CR to the line.
    log(line.endsWith('\r') ? line.slice(0, -1) : line)
  }
  return held + text.slice(start)
}

interface CommandFailure {
  err: ExecCommandError
  command: ProjectCommand
  startTime: [number, number]
  child: TrackedChild | undefined
}

async function handleCommandFailure (ctx: ExecRunContext, { err, command, startTime, child }: CommandFailure): Promise<TaskCompletion> {
  const { opts, params } = ctx
  const { prefix } = command
  // A command that failed after a signal reached pnpm still ends
  // the run as an interrupted one, whatever its own exit status.
  ctx.interruptedBy ??= signalReaching(child)
  if (isErrorCommandNotFound(params[0], err, prefix, command.prependPaths)) {
    err.message = `Command "${params[0]}" not found`
    err.hint = await createExecCommandNotFoundHint(params[0], {
      implicitlyFellbackFromRun: opts.implicitlyFellbackFromRun ?? false,
      dir: opts.dir,
      workspaceDir: opts.workspaceDir,
      modulesDir: opts.modulesDir ?? 'node_modules',
    })
  } else if (!opts.recursive && typeof err.exitCode === 'number') {
    ctx.exitCode = err.exitCode
    return 'passed'
  }
  logger.info(err as ExecCommandError & { prefix: string })

  ctx.result[prefix] = {
    status: 'failure',
    duration: getExecutionDuration(startTime),
    error: err,
    message: err.message,
    prefix,
  }

  if (opts.bail && ctx.firstError == null) {
    if (!err.code?.startsWith('ERR_PNPM_')) {
      err.code = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
    }
    err.prefix = prefix
    ctx.firstError = err
  }
  return 'failed'
}
