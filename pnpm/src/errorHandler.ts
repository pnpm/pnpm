import { promisify } from 'util'
import { logger } from '@pnpm/logger'
import pidTree from 'pidtree'
import { type Global, REPORTER_INITIALIZED } from './main'

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
  await killProcesses('errno' in error && typeof error.errno === 'number' ? error.errno : 1)
}

async function killProcesses (status: number): Promise<void> {
  try {
    const descendentProcesses = await getDescendentProcesses(process.pid)
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
  process.exit(status)
}
