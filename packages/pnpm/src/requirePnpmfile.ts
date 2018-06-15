import logger from '@pnpm/logger'
import chalk from 'chalk'
import path = require('path')

export default (pnpmFilePath: string) => {
  try {
    const pnpmfile = require(pnpmFilePath)
    logger.info(`Using hooks from: ${pnpmFilePath}`)
    if (pnpmfile && pnpmfile.hooks && pnpmfile.hooks.readPackage && typeof pnpmfile.hooks.readPackage !== 'function') {
      throw new TypeError('hooks.readPackage should be a function')
    }
    if (pnpmfile && pnpmfile.hooks && pnpmfile.hooks.readPackage) {
      const readPackage = pnpmfile.hooks.readPackage
      pnpmfile.hooks.readPackage = function (...args: any[]) { // tslint:disable-line
        const newPkg = readPackage(...args)
        if (!newPkg) {
          const err = new Error(`readPackage hook did not return a package manifest object. Hook imported via ${pnpmFilePath}`)
           // tslint:disable:no-string-literal
          err['code'] = 'ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT'
          err['pnpmfile'] = pnpmFilePath
           // tslint:enable:no-string-literal
          throw err
        }
        return newPkg
      }
    }
    pnpmfile.filename = pnpmFilePath
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
