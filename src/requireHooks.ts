import logger from '@pnpm/logger'
import chalk from 'chalk'
import path = require('path')
import {PnpmOptions} from 'supi'

export default function requireHooks (prefix: string, opts: PnpmOptions) {
  if (opts.ignorePnpmfile) {
    return {}
  }
  try {
    const pnpmFilePath = path.join(prefix, 'pnpmfile.js')
    const pnpmFile = require(pnpmFilePath)
    const hooks = pnpmFile && pnpmFile.hooks
    if (!hooks) return {}
    if (hooks.readPackage) {
      if (typeof hooks.readPackage !== 'function') {
        throw new TypeError('hooks.readPackage should be a function')
      }
      logger.info('readPackage hook is declared. Manifests of dependencies might get overridden')
    }
    return hooks
  } catch (err) {
    if (err instanceof SyntaxError) {
      console.error(chalk.red('A syntax error in the pnpmfile.js\n'))
      console.error(err)
      process.exit(1)
      return
    }
    if (err.code !== 'MODULE_NOT_FOUND') throw err
    return {}
  }
}
