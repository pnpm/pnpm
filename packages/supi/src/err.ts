import logger from '@pnpm/logger'

export default function err (error: Error) {
  // bole passes only the name, message and stack of an error
  // that is why we pass error as a message as well, to pass
  // any additional info
  logger.error(error, error)
  process.exit(1)
}
