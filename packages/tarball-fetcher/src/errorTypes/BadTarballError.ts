import PnpmError from '@pnpm/error'

export default class BadTarballError extends PnpmError {
  public expectedSize: number
  public receivedSize: number
  constructor (
    opts: {
      expectedSize: number,
      receivedSize: number,
      tarballUrl: string,
    }
  ) {
    const message = `Actual size (${opts.receivedSize}) of tarball (${opts.tarballUrl}) did not match the one specified in 'Content-Length' header (${opts.expectedSize})`
    super('BAD_TARBALL_SIZE', message)
    this.expectedSize = opts.expectedSize
    this.receivedSize = opts.receivedSize
  }
}
