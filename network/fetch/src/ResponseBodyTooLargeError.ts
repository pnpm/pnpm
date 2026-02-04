import { PnpmError } from '@pnpm/error'

export class ResponseBodyTooLargeError extends PnpmError {
  public readonly url: string
  public readonly maxSize: number
  public readonly receivedSize: number

  constructor (opts: { url: string, maxSize: number, receivedSize: number }) {
    const maxSizeMB = Math.round(opts.maxSize / (1024 * 1024))
    const receivedSizeMB = Math.round(opts.receivedSize / (1024 * 1024))
    super(
      'RESPONSE_BODY_TOO_LARGE',
      `Response body size ${receivedSizeMB} MB exceeds maximum allowed size ${maxSizeMB} MB for ${opts.url}`
    )
    this.url = opts.url
    this.maxSize = opts.maxSize
    this.receivedSize = opts.receivedSize
  }
}
