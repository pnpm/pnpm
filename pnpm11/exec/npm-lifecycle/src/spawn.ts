import { spawn as spawnProcess, type SpawnOptions, type StdioOptions } from 'node:child_process'
import { EventEmitter } from 'node:events'
import type { Readable, Writable } from 'node:stream'

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
  const raw = spawnProcess(cmd, args, spawnOptions)
  const cooked = new EventEmitter() as LifecycleChildProcess

  raw.on('error', (er: SpawnError) => {
    stop()
    er.file = cmd
    cooked.emit('error', er)
  }).on('close', (code, signal) => {
    stop()
    // A shell reports a command it could not find as exit code 127 without
    // an `error` event, so it is reported the way a failed spawn is.
    if (code === 127) {
      const er: SpawnError = new Error('spawn ENOENT')
      er.code = 'ENOENT'
      er.errno = 'ENOENT' as unknown as number
      er.syscall = 'spawn'
      er.file = cmd
      cooked.emit('error', er)
    } else {
      cooked.emit('close', code, signal)
    }
  })

  cooked.stdin = raw.stdin
  cooked.stdout = raw.stdout
  cooked.stderr = raw.stderr
  cooked.pid = raw.pid
  cooked.kill = (signal) => raw.kill(signal)

  return cooked
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
