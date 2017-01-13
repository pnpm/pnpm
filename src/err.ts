import bole = require('bole')

const logger = bole('pnpm')

export default function err (error: Error) {
  logger.error(error)
  process.exit(1)
}
