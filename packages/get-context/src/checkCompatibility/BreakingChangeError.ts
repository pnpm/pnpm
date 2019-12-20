import PnpmError from '@pnpm/error'
import ErrorRelatedSources from './ErrorRelatedSources'

export type BreakingChangeErrorOptions = ErrorRelatedSources & {
  code: string,
  message: string,
}

export default class BreakingChangeError extends PnpmError {
  public relatedIssue?: number
  public relatedPR?: number
  public additionalInformation?: string
  constructor (opts: BreakingChangeErrorOptions) {
    super(opts.code, opts.message)
    this.relatedIssue = opts.relatedIssue
    this.relatedPR = opts.relatedPR
    this.additionalInformation = opts.additionalInformation
  }
}
