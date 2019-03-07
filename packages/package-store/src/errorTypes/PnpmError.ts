export type PnpmErrorCode = 'ERR_PNPM_UNEXPECTED_STORE'
| 'ERR_PNPM_STORE_BREAKING_CHANGE'
| 'ERR_PNPM_MODULES_BREAKING_CHANGE'
| 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE'
| 'ERR_PNPM_MODIFIED_DEPENDENCY'
| 'ERR_PNPM_NO_OFFLINE_META'

export default class PnpmError extends Error {
  public code: PnpmErrorCode
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
}
