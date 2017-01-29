export type PnpmErrorCode = 'UNEXPECTED_STORE' | 'STORE_BREAKING_CHANGE' | 'MODULES_BREAKING_CHANGE'

export class PnpmError extends Error {
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this._code = code
  }

  private _code: PnpmErrorCode
  get code () { return this._code }
}
