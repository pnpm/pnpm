import PnpmError from '@pnpm/error'

export default class UnexpectedStoreError extends PnpmError {
  public expectedStorePath: string
  public actualStorePath: string
  public modulesDir: string
  constructor (
    opts: {
      expectedStorePath: string
      actualStorePath: string
      modulesDir: string
    }
  ) {
    super('UNEXPECTED_STORE', 'Unexpected store location')
    this.expectedStorePath = opts.expectedStorePath
    this.actualStorePath = opts.actualStorePath
    this.modulesDir = opts.modulesDir
  }
}
