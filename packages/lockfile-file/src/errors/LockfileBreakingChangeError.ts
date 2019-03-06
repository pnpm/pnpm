import PnpmError from './PnpmError'

export default class LockfileBreakingChangeError extends PnpmError {
  public filename: string
  constructor (filename: string) {
    super('ERR_PNPM_LOCKFILE_BREAKING_CHANGE', `Lockfile ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
}
