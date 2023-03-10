import { promisify } from 'util'
import { logger } from '@pnpm/logger'
import { REPORTER_INITIALIZED } from './main'
import killcb from 'tree-kill'

const kill = promisify(killcb) as (pid: number, signal: string) => Promise<void>

export async function errorHandler (error: Error & { code?: string }) {
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
    await kill(process.pid, 'SIGINT')
  }, 0)
}
