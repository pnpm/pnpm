import { PnpmError } from '@pnpm/error'

export class LockfileBreakingChangeError extends PnpmError {
  public filename: string
  constructor (filename: string) {
    super('LOCKFILE_BREAKING_CHANGE', `Lockfile ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
}
