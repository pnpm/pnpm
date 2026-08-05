import { spawn } from 'node:child_process'
import path from 'node:path'

export interface TrackableChildProcess {
  pid?: number
  once: (event: 'close' | 'error', listener: () => void) => unknown
}

const trackedChildPids = new Set<number>()

/**
 * Registers a child process spawned for a user command so that
 * killTrackedProcessTrees() can terminate its process tree if pnpm exits
 * while the command is still running.
 */
export function trackChildProcess (child: TrackableChildProcess): void {
  const pid = child.pid
  if (pid == null) return
  trackedChildPids.add(pid)
  const untrack = () => {
    trackedChildPids.delete(pid)
  }
  child.once('close', untrack)
  child.once('error', untrack)
}

/**
 * Kills the process trees of the still-running child processes registered
 * with trackChildProcess(). Best-effort: children that exited concurrently
 * are skipped silently.
 *
 * On Windows the tree is killed with `taskkill /T`, which terminates every
 * descendant of a known PID without enumerating the system process list (an
 * enumeration needs `wmic` or PowerShell there and can take tens of seconds).
 * On POSIX only the tracked child itself is signalled; descendants are
 * expected to be handled by the caller (the error handler enumerates them
 * cheaply with one `ps` call).
 */
export async function killTrackedProcessTrees (): Promise<void> {
  await Promise.all(Array.from(trackedChildPids, killProcessTree))
}

async function killProcessTree (pid: number): Promise<void> {
  if (process.platform === 'win32') {
    // Resolve taskkill to its absolute System32 location so a taskkill.exe
    // planted in the current directory or on PATH can't be run in its place
    // during error cleanup. `process.env` is case-insensitive on Windows, so
    // `SystemRoot` also matches the SYSTEMROOT/systemroot spellings.
    const taskkillPath = path.join(process.env.SystemRoot ?? process.env.windir ?? 'C:\\Windows', 'System32', 'taskkill.exe')
    await new Promise<void>((resolve) => {
      const taskkill = spawn(taskkillPath, ['/pid', pid.toString(), '/T', '/F'], { stdio: 'ignore', windowsHide: true })
      // pnpm is on its error-exit path and must not hang, so the wait is
      // bounded by a timer that resolves even if taskkill never emits 'exit'
      // or 'error' (and reaps a stuck taskkill). The kill is best-effort
      // either way. The timer is unref'd so it can't keep the process alive.
      const timer = setTimeout(() => {
        taskkill.kill()
        resolve()
      }, 10_000)
      timer.unref()
      const done = (): void => {
        clearTimeout(timer)
        resolve()
      }
      // A non-zero exit code (128 when the process is already gone, 1 when
      // access is denied) is deliberately ignored.
      taskkill.once('error', done)
      taskkill.once('exit', done)
    })
  } else {
    try {
      process.kill(pid)
    } catch {
      // the process exited before it could be signalled
    }
  }
}
