import logger from '@pnpm/logger'

export default function err (error: Error) {
  if (!global['reporterInitialized']) {
    console.log(error)
    process.exit(1)
  }
  if (global['reporterInitialized'] === 'silent') {
    process.exit(1)
  }

  // bole passes only the name, message and stack of an error
  // that is why we pass error as a message as well, to pass
  // any additional info
  logger.error(error, error)

  // Differing exit. Otherwise, the reporter wouldn't show the error
  setTimeout(() => process.exit(1), 0)
}
