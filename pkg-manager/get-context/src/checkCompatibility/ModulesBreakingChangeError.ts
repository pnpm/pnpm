import type { ErrorRelatedSources } from '@pnpm/types'
import { BreakingChangeError } from './BreakingChangeError.js'

export type ModulesBreakingChangeErrorOptions = ErrorRelatedSources & {
  modulesPath: string
}

export class ModulesBreakingChangeError extends BreakingChangeError {
  public modulesPath: string
  constructor(opts: ModulesBreakingChangeErrorOptions) {
    super({
      additionalInformation: opts.additionalInformation,
      code: 'MODULES_BREAKING_CHANGE',
      message: `The node_modules structure at "${opts.modulesPath}" is not compatible with the current pnpm version. Run "pnpm install --force" to recreate node_modules.`,
      relatedIssue: opts.relatedIssue,
      relatedPR: opts.relatedPR,
    })

    this.modulesPath = opts.modulesPath
  }
}
