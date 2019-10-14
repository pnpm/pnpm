import PnpmError from '@pnpm/error'

export default class UnexpectedVirtualStoreDirError extends PnpmError {
  public expectedVirtualStoreDir: string
  public actualVirtualStoreDir: string
  public modulesDir: string
  constructor (
    opts: {
      expectedVirtualStoreDir: string,
      actualVirtualStoreDir: string,
      modulesDir: string,
    },
  ) {
    super('UNEXPECTED_VIRTUAL_STORE', 'Unexpected virtual store location')
    this.expectedVirtualStoreDir = opts.expectedVirtualStoreDir
    this.actualVirtualStoreDir = opts.actualVirtualStoreDir
    this.modulesDir = opts.modulesDir
  }
}
