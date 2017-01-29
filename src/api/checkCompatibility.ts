import {stripIndent, oneLine} from 'common-tags'
import {Modules} from '../fs/modulesController'
import {PnpmError, PnpmErrorCode} from '../errorTypes'
import semver = require('semver')

export type ProjectCompatibilityOptions = {
  storePath: string,
  modulesPath: string,
}

export default function checkCompatibility (
  modules: Modules,
  opts: ProjectCompatibilityOptions
) {
  if (modules.storePath !== opts.storePath) {
    throw new UnexpectedStoreError({
      expectedStorePath: modules.storePath,
      actualStorePath: opts.storePath,
    })
  }
  if (!modules.packageManager) {
    throw new ModulesBreakingChangeError({
      modulesPath: opts.modulesPath,
      additionalInformation: 'The change was needed to allow machine stores and dependency locks',
      relatedPR: 524,
    })
  }
  const pnpmVersion = getPackageManagerVersion(modules.packageManager)
  check(pnpmVersion, opts.storePath, opts.modulesPath)
}

function getPackageManagerVersion(packageManager: string) {
  // handle the case when the package is scoped: @scope/pkgname
  if (packageManager.startsWith('@')) {
    return packageManager.split('@')[2]
  } else {
    return packageManager.split('@')[1]
  }
}

function check (pnpmVersion: string, storePath: string, modulesPath: string) {
  if (!pnpmVersion || semver.lt(pnpmVersion, '0.28.0')) {
    throw new StoreBreakingChangeError({
      storePath,
      relatedIssue: 276,
    })
  }
  if (semver.lt(pnpmVersion, '0.33.0')) {
    throw new StoreBreakingChangeError({
      storePath,
      additionalInformation: 'The change was needed to fix the GitHub rate limit issue',
      relatedIssue: 361,
      relatedPR: 363,
    })
  }
  if (semver.lt(pnpmVersion, '0.37.0')) {
    throw new StoreBreakingChangeError({
      storePath,
      additionalInformation: 'The structure of store.json/dependencies was changed to map dependencies to their fullnames',
    })
  }
  if (semver.lt(pnpmVersion, '0.38.0')) {
    throw new StoreBreakingChangeError({
      storePath,
      additionalInformation: 'The structure of store.json/dependencies was changed to not include the redundunt package.json at the end',
    })
  }
  if (!pnpmVersion || semver.lt(pnpmVersion, '0.48.0')) {
    throw new ModulesBreakingChangeError({ modulesPath, relatedPR: 534 })
  }
  if (semver.lt(pnpmVersion, '0.51.0')) {
    throw new ModulesBreakingChangeError({ modulesPath, relatedPR: 576 })
  }
}

type UnexpectedStoreErrorOptions = {
  expectedStorePath: string,
  actualStorePath: string,
}

class UnexpectedStoreError extends PnpmError {
  constructor (opts: UnexpectedStoreErrorOptions) {
    super('UNEXPECTED_STORE', 'Unexpected store used for installation')
    this._expectedStorePath = opts.expectedStorePath
    this._actualStorePath = opts.actualStorePath
  }

  private _expectedStorePath: string
  get expectedStorePath () { return this._expectedStorePath }

  private _actualStorePath: string
  get actualStorePath () { return this._actualStorePath }
}

type BreakingChangeErrorOptions = ErrorRelatedSources & {
  code: PnpmErrorCode,
  message: string,
}

type ErrorRelatedSources = {
  additionalInformation?: string,
  relatedIssue?: number,
  relatedPR?: number,
}

class BreakingChangeError extends PnpmError {
  constructor (opts: BreakingChangeErrorOptions) {
    super(opts.code, opts.message)
    this._relatedIssue = opts.relatedIssue
    this._relatedPR = opts.relatedPR
  }

  private _relatedIssue?: number
  get relatedIssue () { return this._relatedIssue }

  private _relatedPR?: number
  get relatedPR () { return this._relatedPR }
}

type StoreBreakingChangeErrorOptions = ErrorRelatedSources & {
  storePath: string,
}

class StoreBreakingChangeError extends BreakingChangeError {
  constructor (opts: StoreBreakingChangeErrorOptions) {
    super({
      code: 'STORE_BREAKING_CHANGE',
      message: `The store structure was changed. Try running the same command with the --force parameter.`,
      additionalInformation: opts.additionalInformation,
      relatedIssue: opts.relatedIssue,
      relatedPR: opts.relatedPR,
    })
    this._storePath = opts.storePath
  }

  private _storePath: string
  get storePath () { return this._storePath }
}

type ModulesBreakingChangeErrorOptions = ErrorRelatedSources & {
  modulesPath: string,
}

class ModulesBreakingChangeError extends BreakingChangeError {
  constructor (opts: ModulesBreakingChangeErrorOptions) {
    super({
      code: 'MODULES_BREAKING_CHANGE',
      message: `The node_modules structure at ${opts.modulesPath} was changed. Try running the same command with the --force parameter.`,
      additionalInformation: opts.additionalInformation,
      relatedIssue: opts.relatedIssue,
      relatedPR: opts.relatedPR,
    })
    this._modulesPath = opts.modulesPath
  }

  private _modulesPath: string
  get modulesPath () { return this._modulesPath }
}
