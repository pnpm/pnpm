import { PnpmError } from '../../errorTypes'

export default class UnexpectedStoreError extends PnpmError {
  public expectedStorePath: string
  public actualStorePath: string
  constructor (
    opts: {
      expectedStorePath: string,
      actualStorePath: string,
    },
  ) {
    super('ERR_PNPM_UNEXPECTED_STORE', 'Unexpected store used for installation')
    this.expectedStorePath = opts.expectedStorePath
    this.actualStorePath = opts.actualStorePath
  }
}
