export type PnpmErrorCode = 'UNEXPECTED_STORE'
  | 'STORE_BREAKING_CHANGE'
  | 'MODULES_BREAKING_CHANGE'
  | 'MODIFIED_DEPENDENCY'

export class PnpmError extends Error {
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
  code: PnpmErrorCode
}
