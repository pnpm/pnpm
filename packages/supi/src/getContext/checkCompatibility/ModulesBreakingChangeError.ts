import BreakingChangeError from './BreakingChangeError'
import ErrorRelatedSources from './ErrorRelatedSources'

export type ModulesBreakingChangeErrorOptions = ErrorRelatedSources & {
  modulesPath: string,
}

export default class ModulesBreakingChangeError extends BreakingChangeError {
  public modulesPath: string
  constructor (opts: ModulesBreakingChangeErrorOptions) {
    super({
      additionalInformation: opts.additionalInformation,
      code: 'MODULES_BREAKING_CHANGE',
      message: `The node_modules structure at ${opts.modulesPath} was changed. Try running the same command with the --force parameter.`,
      relatedIssue: opts.relatedIssue,
      relatedPR: opts.relatedPR,
    })
    this.modulesPath = opts.modulesPath
  }
}
