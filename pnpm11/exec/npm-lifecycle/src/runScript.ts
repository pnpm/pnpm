import fs from 'node:fs'
import path from 'node:path'
import { PassThrough, type Readable } from 'node:stream'

import byline from '@pnpm/byline'
import { PnpmError } from '@pnpm/error'
import { npath } from '@yarnpkg/fslib'
import { execute } from '@yarnpkg/shell'

import type { Callback, LifecycleError, LifecycleLog, LifecyclePackage, ScriptRun } from './lifecycleTypes.js'
import { missingScriptShellError, SCRIPT_SHELL_NOT_FOUND } from './missingScriptShell.js'
import { relaySignals, type SignalRelay } from './signals.js'
import type { LifecycleChildProcess, SpawnError } from './spawn.js'

export function logId (pkg: LifecyclePackage, stage: string): string {
  return `${pkg._id}~${stage}:`
}

export function runEmulated (run: ScriptRun, cb: Callback): void {
  const { cmd, pkg, stage, wd, env, opts } = run
  const execOpts: Parameters<typeof execute>[2] = { cwd: npath.toPortablePath(wd), env }
  if (opts.stdio === 'pipe') {
    Object.assign(execOpts, createLoggedOutput(run))
  }
  const procError = createProcError(run, cb)
  const finish = async (err?: LifecycleError): Promise<void> => {
    try {
      await run.relayReservation?.settle()
    } catch (settleError: unknown) {
      procError(settleError as LifecycleError)
      return
    }
    procError(err)
  }
  void execute(cmd, [], execOpts)
    .then((code) => {
      opts.log.silly('lifecycle', logId(pkg, stage), 'Returned: code:', code)
      return finish(createExitStatusError(code))
    }, (err: LifecycleError) => finish(err))
}

function createLoggedOutput (run: ScriptRun): { stdout: PassThrough, stderr: PassThrough } {
  const stdout = new PassThrough()
  const stderr = new PassThrough()
  logOutputLines(run, { stream: stdout, name: 'stdout' })
  logOutputLines(run, { stream: stderr, name: 'stderr' })
  return { stdout, stderr }
}

function createExitStatusError (code: number): LifecycleError | undefined {
  if (!code) return undefined
  const er: LifecycleError = new Error(`Exit status ${code}`)
  er.errno = code
  return er
}

interface ScriptOutput {
  stream: Readable
  name: 'stdout' | 'stderr'
}

function logOutputLines (run: ScriptRun, { stream, name }: ScriptOutput): void {
  const { pkg, stage, opts } = run
  byline(stream).on('data', (data: Buffer) => {
    opts.log.verbose('lifecycle', logId(pkg, stage), name, data.toString())
  })
}

export interface SpawnedScript {
  proc: LifecycleChildProcess
  /** The script leads a process group of its own, which is what pnpm signals. */
  ownProcessGroup: boolean
}

/** What became of a spawned script, as its handlers learn it. */
interface SpawnedScriptOutcome {
  /** Set once an `onSpawn` observer threw, with what it threw. */
  spawnObserverFailure?: { error: LifecycleError | undefined }
  deathSignal: NodeJS.Signals | null
}

/**
 * Wait for the spawned script, relaying pnpm's own signals to it meanwhile.
 *
 * `cb` runs at most once, after the script has ended and its output has
 * been reported. It gets no error for a clean exit, and a `LifecycleError`
 * for a failed spawn, a non-zero exit, or an `onSpawn` observer that threw.
 * A child with a process group of its own is signalled as a group, and
 * after a relayed signal `cb` waits for that group as well: the shell may
 * have died from the signal while the script it started is still shutting
 * down. A script killed by a signal makes pnpm raise that signal on itself
 * once every child running alongside it has settled, which ends pnpm before
 * `cb` unless something handles the signal; `cb` then gets a `LifecycleError`.
 */
export function runSpawned (run: ScriptRun, spawned: SpawnedScript, cb: Callback): void {
  const { pkg, stage, opts } = run
  const { proc, ownProcessGroup } = spawned
  const outcome: SpawnedScriptOutcome = { deathSignal: null }
  const relay = relaySignals(proc, {
    ownProcessGroup,
    raiseOnInterrupt: opts.raiseOnInterrupt,
    terminateOnExit: true,
  })
  run.relayReservation?.release()

  // A script killed by a signal makes pnpm raise that signal on itself, so
  // the shell reports an interrupted command rather than a plain failure.
  // That comes after the wait for the script's process group: the raise
  // ends pnpm, and a shell that died from a relayed signal may have left
  // the script still shutting down.
  const finish = createSpawnedScriptFinish({ relay, outcome, procError: createProcError(run, cb) })

  proc.on('error', (err: SpawnError) => {
    finish(outcome.spawnObserverFailure ? outcome.spawnObserverFailure.error : missingScriptShellError(err, opts.scriptShell, run.wd) ?? err)
  })
  proc.on('close', (code: number | null, signal: NodeJS.Signals | null) => {
    opts.log.silly('lifecycle', logId(pkg, stage), 'Returned: code:', code, ' signal:', signal)
    finish(createCloseError(outcome, code, signal))
  })
  // Inherited streams are null on the child; only piped output is reported.
  if (proc.stdout) logOutputLines(run, { stream: proc.stdout, name: 'stdout' })
  if (proc.stderr) logOutputLines(run, { stream: proc.stderr, name: 'stderr' })

  try {
    opts.onSpawn?.(proc)
  } catch (err: unknown) {
    outcome.spawnObserverFailure = { error: err as LifecycleError }
    relay.terminate()
  }
}

function createCloseError (outcome: SpawnedScriptOutcome, code: number | null, signal: NodeJS.Signals | null): LifecycleError | undefined {
  if (outcome.spawnObserverFailure) return outcome.spawnObserverFailure.error
  if (signal) {
    const err: LifecycleError = new PnpmError('CHILD_PROCESS_FAILED', `Command failed with signal "${signal}"`)
    err.signal = signal
    outcome.deathSignal = signal
    return err
  }
  if (code) {
    const err: LifecycleError = new PnpmError('CHILD_PROCESS_FAILED', `Exit status ${code}`)
    err.errno = code
    return err
  }
  return undefined
}

interface SpawnedScriptFinishOptions {
  relay: SignalRelay
  outcome: SpawnedScriptOutcome
  procError: Callback
}

function createSpawnedScriptFinish ({ relay, outcome, procError }: SpawnedScriptFinishOptions): Callback {
  let finishing = false
  return (er) => {
    if (finishing) return
    finishing = true
    let raiseError: LifecycleError | undefined
    if (outcome.deathSignal) {
      void relay.raise(outcome.deathSignal).catch((err: LifecycleError) => {
        raiseError = err
      })
    }
    relay.settle().then(() => {
      procError(raiseError ?? er)
    }, (err: LifecycleError) => procError(raiseError ?? err))
  }
}

/**
 * Wraps `cb` so that it runs once, with the failure of the script decorated
 * with the script's identity and the `ELIFECYCLE` code.
 */
function createProcError (run: ScriptRun, cb: Callback): Callback {
  const cmd = run.shownCmd ?? run.cmd
  let completed = false
  return (er) => {
    if (completed) return
    completed = true
    if (er) decorateLifecycleError(er, { ...run, cmd })
    cb(er)
  }
}

function decorateLifecycleError (er: LifecycleError, run: ScriptRun): void {
  const { cmd, pkg, stage, opts } = run
  opts.log.info('lifecycle', logId(pkg, stage), `Failed to exec ${stage} script`)
  er.message = `${pkg._id} ${stage}: \`${cmd}\`\n${er.message}`
  if (er.code !== 'EPERM' && er.code !== SCRIPT_SHELL_NOT_FOUND) {
    er.code = 'ELIFECYCLE'
  }
  warnIfNodeModulesMissing(opts.dir, opts.log)
  er.pkgid = pkg._id
  er.stage = stage
  er.script = cmd
  er.pkgname = pkg.name
}

function warnIfNodeModulesMissing (dir: string, log: LifecycleLog): void {
  fs.stat(dir, (statError) => {
    if (statError?.code === 'ENOENT' && dir.split(path.sep).slice(-1)[0] === 'node_modules') {
      log.warn('', 'Local package.json exists, but node_modules missing, did you mean to install?')
    }
  })
}
