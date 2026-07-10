import { spawn } from 'node:child_process'

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
    await new Promise<void>((resolve) => {
      // The timeout bounds the wait in case taskkill itself hangs; the kill
      // is best-effort either way, and pnpm is exiting on an error already.
      const taskkill = spawn('taskkill', ['/pid', pid.toString(), '/T', '/F'], { stdio: 'ignore', windowsHide: true, timeout: 10_000 })
      // A non-zero exit code (128 when the process is already gone, 1 when
      // access is denied) is deliberately ignored.
      taskkill.once('error', () => {
        resolve()
      })
      taskkill.once('exit', () => {
        resolve()
      })
    })
  } else {
    try {
      process.kill(pid)
    } catch {
      // the process exited before it could be signalled
    }
  }
}
