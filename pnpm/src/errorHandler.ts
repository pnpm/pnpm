import { promisify } from 'node:util'

import { logger } from '@pnpm/logger'
import pidTree from 'pidtree'

import { exit } from './exit.js'
import { type Global, REPORTER_INITIALIZED } from './main.js'

declare const global: Global

const getDescendentProcesses = promisify((pid: number, callback: (error: Error | undefined, result: number[]) => void) => {
  pidTree(pid, { root: false }, callback)
})

export async function errorHandler (error: Error & { code?: string }): Promise<void> {
  if (error.name != null && error.name !== 'pnpm' && !error.name.startsWith('pnpm:')) {
    try {
      error.name = 'pnpm'
    } catch {
      // Sometimes the name property is read-only
    }
  }

  if (!global[REPORTER_INITIALIZED]) {
    // print parseable error on unhandled exception
    console.log(JSON.stringify({
      error: {
        code: error.code ?? error.name,
        message: error.message,
      },
    }, null, 2))
  } else if (global[REPORTER_INITIALIZED] !== 'silent') {
    // bole passes only the name, message and stack of an error
    // that is why we pass error as a message as well, to pass
    // any additional info
    logger.error(error, error)

    // Deferring exit. Otherwise, the reporter wouldn't show the error
    await new Promise<void>((resolve) => setTimeout(() => {
      resolve()
    }, 0))
  }
  await killProcesses(
    error && typeof error === 'object' && 'errno' in error && typeof error.errno === 'number'
      ? error.errno
      : 1
  )
}

// Enumerating descendant processes shells out to the OS process list. On
// Windows that means `wmic` (and, where it's been removed, a PowerShell
// `Get-CimInstance Win32_Process` fallback), which can take tens of seconds
// on a busy machine — long enough to dominate the exit time of every failed
// command. Bound the lookup so a pathologically slow enumeration can't stall
// the exit; `exit()` calls `process.exit`, which abandons the still-running
// query (a harmless read-only process listing).
const DESCENDANT_LOOKUP_TIMEOUT = 2000

async function killProcesses (status: number): Promise<void> {
  try {
    const descendentProcesses = await Promise.race([
      getDescendentProcesses(process.pid).catch(() => [] as number[]),
      new Promise<number[]>((resolve) => {
        setTimeout(() => resolve([]), DESCENDANT_LOOKUP_TIMEOUT).unref()
      }),
    ])
    for (const pid of descendentProcesses) {
      try {
        process.kill(pid)
      } catch {
        // ignore error here
      }
    }
  } catch {
    // ignore error here
  }
  await exit(status)
}
