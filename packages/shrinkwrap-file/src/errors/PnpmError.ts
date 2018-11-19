// TODO: move to separate package
export type PnpmErrorCode = 'SHRINKWRAP_BREAKING_CHANGE'

export default class PnpmError extends Error {
  public code: PnpmErrorCode
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
}
