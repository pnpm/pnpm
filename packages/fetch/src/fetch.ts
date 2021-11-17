import { requestRetryLogger } from '@pnpm/core-loggers'
import { operation, RetryTimeoutOptions } from '@zkochan/retry'
import fetch, { Request, RequestInit as NodeRequestInit, Response } from 'node-fetch'

export { Response, RetryTimeoutOptions }

interface URLLike {
  href: string
}

export type RequestInfo = string | URLLike | Request

export interface RequestInit extends NodeRequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
}

export const isRedirect = fetch.isRedirect

export default async function fetchRetry (url: RequestInfo, opts: RequestInit = {}): Promise<Response> {
  const retryOpts = opts.retry ?? {}
  const maxRetries = retryOpts.retries ?? 2

  const op = operation({
    factor: retryOpts.factor ?? 10,
    maxTimeout: retryOpts.maxTimeout ?? 60000,
    minTimeout: retryOpts.minTimeout ?? 10000,
    randomize: false,
    retries: maxRetries,
  })

  try {
    return await new Promise((resolve, reject) => op.attempt(async (attempt) => {
      try {
        // this will be retried
        const res = await fetch(url as any, opts) // eslint-disable-line
        // A retry on 409 sometimes helps when making requests to the Bit registry.
        if ((res.status >= 500 && res.status < 600) || [408, 409, 420, 429].includes(res.status)) {
          throw new ResponseError(res)
        } else {
          resolve(res)
          return
        }
      } catch (error: any) { // eslint-disable-line
        const timeout = op.retry(error)
        if (timeout === false) {
          reject(op.mainError())
          return
        }
        requestRetryLogger.debug({
          attempt,
          error,
          maxRetries,
          method: opts.method ?? 'GET',
          timeout,
          url: url.toString(),
        })
      }
    }))
  } catch (err) {
    if (err instanceof ResponseError) {
      return err.res
    }
    throw err
  }
}

export class ResponseError extends Error {
  public res: Response
  public code: number
  public status: number
  public statusCode: number
  public url: string
  constructor (res: Response) {
    super(res.statusText)

    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, ResponseError)
    }

    this.name = this.constructor.name
    this.res = res

    // backward compat
    this.code = this.status = this.statusCode = res.status
    this.url = res.url
  }
}
