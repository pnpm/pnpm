import {stripIndent, oneLine} from 'common-tags'
import {Modules} from '../fs/modulesController'
import {PnpmError, PnpmErrorCode} from '../errorTypes'
import semver = require('semver')

export default function checkCompatibility (
  modules: Modules,
  opts: {
    storePath: string,
    modulesPath: string,
  }
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
  if (semver.lt(pnpmVersion, '0.52.0')) {
    throw new ModulesBreakingChangeError({ modulesPath, relatedPR: 593 })
  }
  if (semver.lt(pnpmVersion, '0.62.0')) {
    throw new ModulesBreakingChangeError({
      modulesPath,
      relatedPR: 660,
      additionalInformation: 'Information about the node_modules structure is stored in a node_modules/.shrinkwrap.yaml file instead of a node_modules/.graph.yaml file'
    })
  }
  if (semver.lt(pnpmVersion, '0.64.0')) {
    throw new ModulesBreakingChangeError({
      modulesPath,
      relatedPR: 694,
      relatedIssue: 678,
      additionalInformation: 'Packages having peer dependencies are linked to different variations. The variations depend on the set of resolved peer dependencies'
    })
  }
}

class UnexpectedStoreError extends PnpmError {
  constructor (
    opts: {
      expectedStorePath: string,
      actualStorePath: string,
    }
  ) {
    super('UNEXPECTED_STORE', 'Unexpected store used for installation')
    this.expectedStorePath = opts.expectedStorePath
    this.actualStorePath = opts.actualStorePath
  }
  expectedStorePath: string
  actualStorePath: string
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
    this.relatedIssue = opts.relatedIssue
    this.relatedPR = opts.relatedPR
    this.additionalInformation = opts.additionalInformation
  }
  relatedIssue?: number
  relatedPR?: number
  additionalInformation?: string
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
    this.storePath = opts.storePath
  }
  storePath: string
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
    this.modulesPath = opts.modulesPath
  }
  modulesPath: string
}
