import PnpmError from '@pnpm/error'
import logger from '@pnpm/logger'
import { PackageManifest } from '@pnpm/types'
import chalk from 'chalk'
import fs = require('fs')

class BadReadPackageHookError extends PnpmError {
  public readonly pnpmfile: string

  constructor (pnpmfile: string) {
    super('BAD_READ_PACKAGE_HOOK_RESULT', `readPackage hook did not return a package manifest object. Hook imported via ${pnpmfile}`)
    this.pnpmfile = pnpmfile
  }
}

class PnpmFileFailError extends PnpmError {
  public readonly pnpmfile: string
  public readonly originalError: Error

  constructor (pnpmfile: string, originalError: Error) {
    super('PNPMFILE_FAIL', `Error during pnpmfile execution. pnpmfile: "${pnpmfile}". Error: "${originalError.message}".`)
    this.pnpmfile = pnpmfile
    this.originalError = originalError
  }
}

export default (pnpmFilePath: string, prefix: string) => {
  try {
    const pnpmfile = require(pnpmFilePath)
    logger.info({
      message: `Using hooks from: ${pnpmFilePath}`,
      prefix,
    })
    if (pnpmfile?.hooks?.readPackage && typeof pnpmfile.hooks.readPackage !== 'function') {
      throw new TypeError('hooks.readPackage should be a function')
    }
    if (pnpmfile?.hooks?.readPackage) {
      const readPackage = pnpmfile.hooks.readPackage
      pnpmfile.hooks.readPackage = function (pkg: PackageManifest, ...args: any[]) { // tslint:disable-line
        pkg.dependencies = pkg.dependencies || {}
        pkg.devDependencies = pkg.devDependencies || {}
        pkg.optionalDependencies = pkg.optionalDependencies || {}
        pkg.peerDependencies = pkg.peerDependencies || {}
        const newPkg = readPackage(pkg, ...args)
        if (!newPkg) {
          throw new BadReadPackageHookError(pnpmFilePath)
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
    if (err.code !== 'MODULE_NOT_FOUND' || pnpmFileExistsSync(pnpmFilePath)) {
      throw new PnpmFileFailError(pnpmFilePath, err)
    }
    return undefined
  }
}

function pnpmFileExistsSync (pnpmFilePath: string) {
  const pnpmFileRealName = pnpmFilePath.endsWith('.js')
    ? pnpmFilePath
    : `${pnpmFilePath}.js`
  return fs.existsSync(pnpmFileRealName)
}
