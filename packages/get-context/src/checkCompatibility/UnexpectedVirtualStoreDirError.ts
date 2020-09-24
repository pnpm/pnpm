import PnpmError from '@pnpm/error'

export default class UnexpectedVirtualStoreDirError extends PnpmError {
  public expected: string
  public actual: string
  public modulesDir: string
  constructor (
    opts: {
      expected: string
      actual: string
      modulesDir: string
    }
  ) {
    super('UNEXPECTED_VIRTUAL_STORE', 'Unexpected virtual store location')
    this.expected = opts.expected
    this.actual = opts.actual
    this.modulesDir = opts.modulesDir
  }
}
