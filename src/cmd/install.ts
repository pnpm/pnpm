import chalk from 'chalk'
import path = require('path')
import logger from 'pnpm-logger'
import {install, installPkgs, PnpmOptions} from 'supi'

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default function installCmd (input: string[], opts: PnpmOptions) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()
  opts.hooks = requireHooks(prefix)

  if (!input || !input.length) {
    return install(opts)
  }
  return installPkgs(input, opts)
}

function requireHooks (prefix: string) {
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
