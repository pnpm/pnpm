import {stripIndent, oneLine} from 'common-tags'
import {Modules, LAYOUT_VERSION} from '../fs/modulesController'
import {PnpmError, PnpmErrorCode} from '../errorTypes'
import semver = require('semver')
import path = require('path')

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

export default function checkCompatibility (
  modules: Modules,
  opts: {
    storePath: string,
    modulesPath: string,
  }
) {
  // Important: comparing paths with path.relative()
  // is the only way to compare paths correctly on Windows
  // as of Node.js 4-9
  // See related issue: https://github.com/pnpm/pnpm/issues/996
  if (path.relative(modules.store, opts.storePath) !== '') {
    throw new UnexpectedStoreError({
      expectedStorePath: modules.store,
      actualStorePath: opts.storePath,
    })
  }
  if (!modules.layoutVersion || modules.layoutVersion !== LAYOUT_VERSION) {
    throw new ModulesBreakingChangeError({
      modulesPath: opts.modulesPath,
      additionalInformation: 'The change was needed to make `independent-leafs` not the default installation layout',
      relatedIssue: 821,
    })
  }
}
