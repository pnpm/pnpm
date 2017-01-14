import logger from 'pnpm-logger'

export default function err (error: Error) {
  logger.error(error)
  process.exit(1)
}
