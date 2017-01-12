import chalk = require('chalk')
import bole = require('bole')
const logger = bole('pnpm')

type HttpError = Error & {
  host: string,
  path: string,
  method: string
}

export default function err (error: Error) {
  logger.error('', error)
  console.error('')
  if ((<HttpError>error).host && (<HttpError>error).path) {
    const httpError = <HttpError>error
    console.error('' + httpError.message)
    console.error('' + httpError.method + ' ' + httpError.host + httpError.path)
  } else {
    console.error(chalk.red(' ! ' + (error.message || error)))
    if (process.env.DEBUG_PROMISE && error.stack && !error['silent']) {
      console.error(chalk.red(error.stack))
    }
  }
  console.error('')
  process.exit(1)
}
