import { logger } from '@pnpm/logger'
import { REPORTER_INITIALIZED } from './main'

export function errorHandler (error: Error) {
  if (!global[REPORTER_INITIALIZED]) {
    console.log(error)
    process.exitCode = 1
    return
  }
  if (global[REPORTER_INITIALIZED] === 'silent') {
    process.exitCode = 1
    return
  }
  if (error.name != null && error.name !== 'pnpm' && !error.name.startsWith('pnpm:')) {
    try {
      error.name = 'pnpm'
    } catch {
      // Sometimes the name property is read-only
    }
  }

  // bole passes only the name, message and stack of an error
  // that is why we pass error as a message as well, to pass
  // any additional info
  logger.error(error, error)

  // Deferring exit. Otherwise, the reporter wouldn't show the error
  setTimeout(() => process.exit(1), 0)
}
