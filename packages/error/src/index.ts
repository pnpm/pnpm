export default class PnpmError extends Error {
  public readonly code: string
  constructor (code: string, message: string) {
    super(message)
    this.code = `ERR_PNPM_${code}`
  }
}
