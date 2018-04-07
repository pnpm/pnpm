import logger from '@pnpm/logger'
import chalk from 'chalk'
import path = require('path')

export default (pnpmFilePath: string) => {
  try {
    const pnpmfile = require(pnpmFilePath)
    logger.info(`Using hooks from: ${pnpmFilePath}`)
    return pnpmfile
  } catch (err) {
    if (err instanceof SyntaxError) {
      console.error(chalk.red('A syntax error in the pnpmfile.js\n'))
      console.error(err)
      process.exit(1)
      return
    }
    if (err.code !== 'MODULE_NOT_FOUND') throw err
    return undefined
  }
}
