import PnpmError from './PnpmError'

export default class ShrinkwrapBreakingChangeError extends PnpmError {
  public filename: string
  constructor (filename: string) {
    super('SHRINKWRAP_BREAKING_CHANGE', `Shrinkwrap file ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
}
