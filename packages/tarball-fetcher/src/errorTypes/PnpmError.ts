export type PnpmErrorCode = 'ERR_PNPM_NO_OFFLINE_TARBALL'
  | 'ERR_PNPM_BAD_TARBALL_SIZE'
  | 'ERR_PNPM_CORRUPTED_TARBALL'
  | 'ERR_PNPM_BAD_TARBALL_CHECKSUM'

export default class PnpmError extends Error {
  public code: PnpmErrorCode
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
}
