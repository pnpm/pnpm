export default class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string
  public pkgsStack?: Array<{ id: string, name: string, version: string }>
  constructor (code: string, message: string, opts?: { hint: string }) {
    super(message)
    this.code = `ERR_PNPM_${code}`
    this.hint = opts?.hint
  }
}
