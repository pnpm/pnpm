import logger from '@pnpm/logger'

export default function err (error: Error) {
  if (!global['reporterInitialized']) {
    console.log(error)
    process.exitCode = 1
    return
  }
  if (global['reporterInitialized'] === 'silent') {
    process.exitCode = 1
    return
  }
  if (error.name != null && error.name !== 'pnpm' && !error.name.startsWith('pnpm:')) {
    error.name = 'pnpm'
  }

  // bole passes only the name, message and stack of an error
  // that is why we pass error as a message as well, to pass
  // any additional info
  logger.error(error, error)

  // Deferring exit. Otherwise, the reporter wouldn't show the error
  setTimeout(() => process.exit(1), 0)
}
