import { promisify } from 'util'
import { logger } from '@pnpm/logger'
import pidTree from 'pidtree'
import { REPORTER_INITIALIZED } from './main'

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
    process.exitCode = 1
    return
  }
  if (global[REPORTER_INITIALIZED] === 'silent') {
    process.exitCode = 1
    return
  }

  // bole passes only the name, message and stack of an error
  // that is why we pass error as a message as well, to pass
  // any additional info
  logger.error(error, error)

  // Deferring exit. Otherwise, the reporter wouldn't show the error
  setTimeout(async () => {
    await killProcesses('errno' in error && typeof error.errno === 'number' ? error.errno : 1)
  }, 0)
}

async function killProcesses (status: number): Promise<void> {
  try {
    const descendentProcesses = await getDescendentProcesses(process.pid)
    for (const pid of descendentProcesses) {
      try {
        process.kill(pid)
      } catch (err) {
        // ignore error here
      }
    }
  } catch (err) {
    // ignore error here
  }
  process.exit(status)
}
