import nodeFetch, {
  Response,
} from 'node-fetch'

import { operation } from '@zkochan/retry'

import { requestRetryLogger } from '@pnpm/core-loggers'

const NO_RETRY_ERROR_CODES = new Set([
  'SELF_SIGNED_CERT_IN_CHAIN',
  'ERR_OSSL_PEM_NO_START_LINE',
])

export async function fetch(
  url: RequestInfo,
  opts: RequestInit | undefined = {}
): Promise<Response> {
  const retryOpts = opts.retry ?? {}

  const maxRetries = retryOpts.retries ?? 2

  const op = operation({
    factor: retryOpts.factor ?? 10,
    maxTimeout: retryOpts.maxTimeout ?? 60_000,
    minTimeout: retryOpts.minTimeout ?? 10_000,
    randomize: false,
    retries: maxRetries,
  })

  try {
    return await new Promise((resolve: (value: nodeFetch.Response | PromiseLike<nodeFetch.Response>) => void, reject: (reason?: Error | null | undefined) => void): void => {
      op.attempt(async (attempt: number): Promise<void> => {
        try {
          // this will be retried
          const res = await nodeFetch(url as any, opts) // eslint-disable-line

          // A retry on 409 sometimes helps when making requests to the Bit registry.
          if (
            (res.status >= 500 && res.status < 600) ||
            [408, 409, 420, 429].includes(res.status)
          ) {
            throw new ResponseError(res)
          } else {
            resolve(res)
          }
        } catch (error: any) { // eslint-disable-line
          if (error.code && NO_RETRY_ERROR_CODES.has(error.code)) {
            throw error
          }

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
      })
    })
  } catch (err: unknown) {
    if (err instanceof ResponseError) {
      return err.res
    }

    throw err
  }
}

export class ResponseError extends Error {
  public url: string
  public code: number
  public res: Response
  public status: number
  public statusCode: number
  constructor(res: Response) {
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
