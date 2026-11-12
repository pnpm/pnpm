import { type ChildProcess, spawn as spawnProcess, type SpawnOptions, type StdioOptions } from 'node:child_process'
import { EventEmitter } from 'node:events'
import path from 'node:path'
import type { Readable, Writable } from 'node:stream'
import { fileURLToPath } from 'node:url'

import which from 'which'

import { prepareProcessGroupWatchdog, type ProcessGroupWatchdog } from './signals.js'

/**
 * How long a script's piped output is still read once the script has exited.
 * A process the script left running in the background inherits the pipes and
 * can hold them open for as long as it lives
 * ([pnpm/pnpm#5730](https://github.com/pnpm/pnpm/issues/5730)).
 */
const OUTPUT_DRAIN_AFTER_EXIT_MS = 1000

export interface ProgressLog {
  progressEnabled?: boolean
  disableProgress?: () => void
  enableProgress?: () => void
}

export interface SpawnError extends NodeJS.ErrnoException {
  file?: string
}

/**
 * A spawned lifecycle script. It emits `close` with the exit code and signal,
 * and `error` when the script could not be started.
 */
export interface LifecycleChildProcess extends EventEmitter {
  stdin: Writable | null
  stdout: Readable | null
  stderr: Readable | null
  pid?: number
  watchdog?: ProcessGroupWatchdog
  kill: (signal?: NodeJS.Signals | number) => boolean
}

export interface LifecycleSpawnOptions extends SpawnOptions {
  /** Progress output is paused while a script that writes to the terminal runs. */
  log: ProgressLog
}

export function spawn (cmd: string, args: string[], options: LifecycleSpawnOptions): LifecycleChildProcess {
  const { log, ...spawnOptions } = options
  const cmdWillOutput = willCmdOutput(spawnOptions.stdio)

  if (cmdWillOutput) startRunning(log)
  // A child that could not be spawned emits `error` and then `close`, so the
  // progress accounting for it ends on whichever comes first.
  let stopped = !cmdWillOutput
  const stop = (): void => {
    if (stopped) return
    stopped = true
    stopRunning(log)
  }
  const { raw, watchdog } = spawnScript(cmd, args, spawnOptions)
  const cooked = new EventEmitter() as LifecycleChildProcess

  raw.once('exit', () => {
    stopReadingOutputAfterDrain(raw)
  })

  raw.on('error', (er: SpawnError) => {
    stop()
    er.file = cmd
    cooked.emit('error', er)
  }).on('close', (code, signal) => {
    stop()
    // A shell reports a command it could not find as exit code 127 without
    // an `error` event, so it is reported the way a failed spawn is.
    if (code === 127) {
      cooked.emit('error', createCommandNotFoundError(cmd))
    } else {
      cooked.emit('close', code, signal)
    }
  })

  cooked.stdin = raw.stdin
  cooked.stdout = raw.stdout
  cooked.stderr = raw.stderr
  cooked.pid = raw.pid
  cooked.watchdog = watchdog
  cooked.kill = (signal) => raw.kill(signal)

  return cooked
}

function spawnScript (cmd: string, args: string[], options: SpawnOptions): { raw: ChildProcess, watchdog?: ProcessGroupWatchdog } {
  const shell = options.detached && process.platform !== 'win32' ? which.sync('sh', { nothrow: true }) : null
  if (shell == null || !scriptExists(cmd, options)) {
    return { raw: spawnProcess(cmd, args, options) }
  }
  const stdio = scriptStdio(options.stdio)
  // POSIX shells only guarantee single-digit descriptors.
  if (stdio.length > 9) return { raw: spawnProcess(cmd, args, options) }
  const watch = prepareProcessGroupWatchdog()
  const descriptor = stdio.length
  const registration = `printf '+%s\\n' "$$" >&${descriptor} || exit $?; exec ${descriptor}>&-; exec "$@"`
  const raw = spawnProcess(shell, ['-c', registration, shell, cmd, ...args], {
    ...options,
    stdio: [...stdio, watch.lifeline],
  })
  return { raw, watchdog: raw.pid == null ? undefined : watch.watch(raw.pid) }
}

function scriptExists (cmd: string, options: SpawnOptions): boolean {
  const cwd = typeof options.cwd === 'string' ? options.cwd : options.cwd == null ? process.cwd() : fileURLToPath(options.cwd)
  const executable = cmd.includes('/') ? path.resolve(cwd, cmd) : cmd
  const searchPath = (options.env == null ? process.env.PATH : options.env.PATH) ?? '/usr/bin:/bin'
  return which.sync(executable, {
    path: searchPath.split(path.delimiter).map((entry) => path.resolve(cwd, entry)).join(path.delimiter),
    nothrow: true,
  }) != null
}

function scriptStdio (stdio: StdioOptions | undefined): Exclude<StdioOptions, string> {
  if (Array.isArray(stdio)) return [...stdio, ...Array.from({ length: Math.max(0, 3 - stdio.length) }, () => 'pipe' as const)]
  if (stdio === 'inherit') return [0, 1, 2]
  return [stdio ?? 'pipe', stdio ?? 'pipe', stdio ?? 'pipe']
}

function stopReadingOutputAfterDrain (raw: ChildProcess): void {
  const outputTimeout = setTimeout(() => {
    raw.stdout?.destroy()
    raw.stderr?.destroy()
  }, OUTPUT_DRAIN_AFTER_EXIT_MS)
  raw.once('close', () => {
    clearTimeout(outputTimeout)
  })
}

function createCommandNotFoundError (cmd: string): SpawnError {
  const er: SpawnError = new Error('spawn ENOENT')
  er.code = 'ENOENT'
  er.errno = 'ENOENT' as unknown as number
  er.syscall = 'spawn'
  er.file = cmd
  return er
}

function willCmdOutput (stdio: StdioOptions | undefined): boolean {
  if (stdio === 'inherit') return true
  if (!Array.isArray(stdio)) return false
  for (let fh = 1; fh <= 2; ++fh) {
    if (stdio[fh] === 'inherit') return true
    if (stdio[fh] === 1 || stdio[fh] === 2) return true
  }
  return false
}

let progressEnabled: boolean | undefined
let running = 0

function startRunning (log: ProgressLog): void {
  if (progressEnabled == null) progressEnabled = log.progressEnabled
  if (progressEnabled) log.disableProgress?.()
  ++running
}

function stopRunning (log: ProgressLog): void {
  --running
  if (progressEnabled && running === 0) log.enableProgress?.()
}
