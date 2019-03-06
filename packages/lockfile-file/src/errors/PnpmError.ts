// TODO: move to separate package
export type PnpmErrorCode = 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE'

export default class PnpmError extends Error {
  public code: PnpmErrorCode
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
}
