export type PnpmErrorCode = 'NO_OFFLINE_TARBALL' | 'BAD_TARBALL_SIZE'

export default class PnpmError extends Error {
  public code: PnpmErrorCode
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
}
