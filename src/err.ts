import logger from './logger'

export default function err (error: Error) {
  logger.error(error)
  process.exit(1)
}
