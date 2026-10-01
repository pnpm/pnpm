import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { isError, PnpmError } from '@pnpm/error'
import type { CustomFetcher, CustomResolver } from '@pnpm/hooks.types'
import { logger } from '@pnpm/logger'
import type { Finder, PackageManifest } from '@pnpm/types'
import chalk from 'chalk'

import type { Hooks } from './Hooks.js'

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
    const pnpmfile = await loadPnpmfileModule(pnpmFilePath)
    if (typeof pnpmfile === 'undefined') {
      logger.warn({
        message: `Ignoring the pnpmfile at "${pnpmFilePath}". It exports "undefined".`,
        prefix,
      })
      return { pnpmfileModule: undefined }
    }
    wrapReadPackageHook(pnpmfile, pnpmFilePath)
    return { pnpmfileModule: pnpmfile }
  } catch (err: unknown) {
    if (err instanceof SyntaxError) {
      console.error(chalk.red(`A syntax error in the "${pnpmFilePath}"\n`))
      console.error(err)
      // eslint-disable-next-line n/no-process-exit -- a pnpmfile that does not parse aborts pnpm after printing the syntax error
      process.exit(1)
    }
    if (isModuleNotFoundError(err) && !pnpmFileExistsSync(pnpmFilePath)) {
      return undefined
    }
    throw new PnpmFileFailError(pnpmFilePath, toError(err))
  }
}

async function loadPnpmfileModule (pnpmFilePath: string): Promise<Pnpmfile> {
  if (pnpmFilePath.endsWith('.mjs')) {
    const url = pathToFileURL(path.resolve(pnpmFilePath)).href
    return import(url)
  }
  return require(pnpmFilePath)
}

function wrapReadPackageHook (pnpmfile: Pnpmfile, pnpmFilePath: string): void {
  if (!pnpmfile.hooks) return
  if (pnpmfile.hooks.readPackage && typeof pnpmfile.hooks.readPackage !== 'function') {
    throw new TypeError('hooks.readPackage should be a function')
  }
  if (pnpmfile.hooks.beforePacking && typeof pnpmfile.hooks.beforePacking !== 'function') {
    throw new TypeError('hooks.beforePacking should be a function')
  }
  if (pnpmfile.hooks.readPackage) {
    const rawReadPackage = pnpmfile.hooks.readPackage as Function // eslint-disable-line
    pnpmfile.hooks.readPackage = async function (pkg: PackageManifest, ...args: any[]) { // eslint-disable-line
      initializePackageDeps(pkg)
      const newPkg = await rawReadPackage(pkg, ...args)
      validateReadPackageResult(newPkg, pnpmFilePath)
      return newPkg
    }
  }
}

function initializePackageDeps (pkg: PackageManifest): void {
  pkg.dependencies = pkg.dependencies ?? {}
  pkg.devDependencies = pkg.devDependencies ?? {}
  pkg.optionalDependencies = pkg.optionalDependencies ?? {}
  pkg.peerDependencies = pkg.peerDependencies ?? {}
}

const DEPENDENCY_FIELDS = ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'] as const

function validateReadPackageResult (newPkg: PackageManifest, pnpmFilePath: string): void {
  if (!newPkg || typeof newPkg !== 'object' || Array.isArray(newPkg)) {
    throw new BadReadPackageHookError(pnpmFilePath, 'readPackage hook did not return a package manifest object.')
  }
  for (const dep of DEPENDENCY_FIELDS) {
    validateDepField(newPkg, dep, pnpmFilePath)
  }
}

function validateDepField (newPkg: PackageManifest, dep: typeof DEPENDENCY_FIELDS[number], pnpmFilePath: string): void {
  const deps = newPkg[dep]
  if (deps != null && (typeof deps !== 'object' || Array.isArray(deps))) {
    throw new BadReadPackageHookError(pnpmFilePath, `readPackage hook returned package manifest object's property '${dep}' must be an object.`)
  }
  for (const [depName, range] of Object.entries(deps ?? {})) {
    if (typeof range !== 'string') {
      const typeStr = range === null ? 'null' : typeof range
      throw new BadReadPackageHookError(pnpmFilePath, `readPackage hook returned an invalid range for '${depName}' in the '${dep}' of ${describePackage(newPkg)}. Expected a string, got ${typeStr}. To remove the dependency, delete the property.`)
    }
  }
}

function describePackage (pkg: PackageManifest): string {
  if (typeof pkg.name !== 'string' || !pkg.name) return 'an unnamed package'
  return typeof pkg.version === 'string' && pkg.version ? `${pkg.name}@${pkg.version}` : pkg.name
}

/**
 * Errors are matched by shape, not by class, because asynchronous module customization
 * hooks forward them from their own realm, where `util.types.isNativeError()` returns false.
 * See https://github.com/pnpm/pnpm/issues/11701
 */
function isModuleNotFoundError (err: unknown): boolean {
  return typeof err === 'object' && err !== null && 'code' in err &&
    (err.code === 'MODULE_NOT_FOUND' || err.code === 'ERR_MODULE_NOT_FOUND')
}

function pnpmFileExistsSync (pnpmFilePath: string): boolean {
  const pnpmFileRealName = pnpmFilePath.endsWith('.cjs') || pnpmFilePath.endsWith('.mjs')
    ? pnpmFilePath
    : `${pnpmFilePath}.cjs`
  return fs.existsSync(pnpmFileRealName)
}

function toError (err: unknown): Error {
  if (isError(err)) return err
  try {
    return new Error(String(err), { cause: err })
  } catch {
    return new Error('[non-Error value thrown]', { cause: err })
  }
}
