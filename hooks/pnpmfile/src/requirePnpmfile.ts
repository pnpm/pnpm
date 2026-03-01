import assert from 'assert'
import fs from 'fs'
import path from 'path'
import util from 'util'
import { pathToFileURL } from 'url'
import { createRequire } from 'module'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import { type PackageManifest, type Finder } from '@pnpm/types'
import { type CustomResolver, type CustomFetcher } from '@pnpm/hooks.types'
import chalk from 'chalk'
import { type Hooks } from './Hooks.js'

const require = createRequire(import.meta.url)

export class BadReadPackageHookError extends PnpmError {
  public readonly pnpmfile: string

  constructor (pnpmfile: string, message: string) {
    super('BAD_READ_PACKAGE_HOOK_RESULT', `${message} Hook imported via ${pnpmfile}`)
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

export type Finders = Record<string, Finder>

export interface Pnpmfile {
  hooks?: Hooks
  finders?: Finders
  resolvers?: CustomResolver[]
  fetchers?: CustomFetcher[]
}

export async function requirePnpmfile (pnpmFilePath: string, prefix: string): Promise<{ pnpmfileModule: Pnpmfile | undefined } | undefined> {
  try {
    let pnpmfile: Pnpmfile
    // Check if it's an ESM module (ends with .mjs)
    if (pnpmFilePath.endsWith('.mjs')) {
      const url = pathToFileURL(path.resolve(pnpmFilePath)).href
      pnpmfile = await import(url)
    } else {
      // Use require for CommonJS modules
      pnpmfile = require(pnpmFilePath)
    }
    if (typeof pnpmfile === 'undefined') {
      logger.warn({
        message: `Ignoring the pnpmfile at "${pnpmFilePath}". It exports "undefined".`,
        prefix,
      })
      return { pnpmfileModule: undefined }
    }
    if (pnpmfile?.hooks?.readPackage && typeof pnpmfile.hooks.readPackage !== 'function') {
      throw new TypeError('hooks.readPackage should be a function')
    }
    if (pnpmfile?.hooks?.readPackage) {
      const readPackage = pnpmfile.hooks.readPackage as Function // eslint-disable-line
      pnpmfile.hooks.readPackage = async function (pkg: PackageManifest, ...args: any[]) { // eslint-disable-line
        pkg.dependencies = pkg.dependencies ?? {}
        pkg.devDependencies = pkg.devDependencies ?? {}
        pkg.optionalDependencies = pkg.optionalDependencies ?? {}
        pkg.peerDependencies = pkg.peerDependencies ?? {}
        const newPkg = await readPackage(pkg, ...args)
        if (!newPkg) {
          throw new BadReadPackageHookError(pnpmFilePath, 'readPackage hook did not return a package manifest object.')
        }
        const dependencies = ['dependencies', 'optionalDependencies', 'peerDependencies']
        for (const dep of dependencies) {
          if (newPkg[dep] && typeof newPkg[dep] !== 'object') {
            throw new BadReadPackageHookError(pnpmFilePath, `readPackage hook returned package manifest object's property '${dep}' must be an object.`)
          }
        }
        return newPkg
      }
      if (pnpmfile?.hooks?.beforePacking && typeof pnpmfile.hooks.beforePacking !== 'function') {
        throw new TypeError('hooks.beforePacking should be a function')
      }
    }
    return { pnpmfileModule: pnpmfile }
  } catch (err: unknown) {
    if (err instanceof SyntaxError) {
      console.error(chalk.red(`A syntax error in the "${pnpmFilePath}"\n`))
      console.error(err)
      process.exit(1)
    }
    assert(util.types.isNativeError(err))
    if (
      !('code' in err && (err.code === 'MODULE_NOT_FOUND' || err.code === 'ERR_MODULE_NOT_FOUND')) ||
      pnpmFileExistsSync(pnpmFilePath)
    ) {
      throw new PnpmFileFailError(pnpmFilePath, err)
    }
    return undefined
  }
}

function pnpmFileExistsSync (pnpmFilePath: string): boolean {
  const pnpmFileRealName = pnpmFilePath.endsWith('.cjs') || pnpmFilePath.endsWith('.mjs')
    ? pnpmFilePath
    : `${pnpmFilePath}.cjs`
  return fs.existsSync(pnpmFileRealName)
}
