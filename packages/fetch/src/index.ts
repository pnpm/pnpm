import { globalWarn } from '@pnpm/logger'
import * as retry from '@zkochan/retry'
import { Request, RequestInit as NodeRequestInit, Response } from 'node-fetch'
import fetch = require('node-fetch-unix')
import prettyMilliseconds = require('pretty-ms')

// retry settings
const MIN_TIMEOUT = 10
const MAX_RETRIES = 5
const MAX_RETRY_AFTER = 20
const FACTOR = 6

export { Response }

export interface RetryOpts {
  factor?: number
  maxTimeout?: number
  minTimeout?: number
  retries?: number
}

interface URLLike {
  href: string
}

export type RequestInfo = string | URLLike | Request

export interface RequestInit extends NodeRequestInit {
  retry?: RetryOpts
}

export const isRedirect = fetch.isRedirect

export default async function fetchRetry (url: RequestInfo, opts: RequestInit = {}): Promise<Response> {
  const retryOpts = Object.assign({
    factor: FACTOR,
    // timeouts will be [10, 60, 360, 2160, 12960]
    // (before randomization is added)
    maxRetryAfter: MAX_RETRY_AFTER,
    minTimeout: MIN_TIMEOUT,
    retries: MAX_RETRIES,
  }, opts.retry)

  const op = retry.operation(retryOpts)

  try {
    return await new Promise((resolve, reject) => op.attempt(async (currentAttempt) => {
      const { method = 'GET' } = opts
      try {
        // this will be retried
        const res = await fetch(url, opts)
        if ((res.status >= 500 && res.status < 600) || res.status === 429) {
          throw new ResponseError(res)
        } else {
          resolve(res)
          return
        }
      } catch (err) {
        const timeout = op.retry(err)
        if (timeout === false) {
          reject(op.mainError())
          return
        }
        const retriesLeft = retryOpts.retries - currentAttempt + 1
        globalWarn(`${method} ${url} error (${err.status ?? err.errno}). ` +
          `Will retry in ${prettyMilliseconds(timeout, { verbose: true })}. ` +
          `${retriesLeft} retries left.`
        )
      }
    }))
  } catch (err) {
    if (err instanceof ResponseError) {
      return err.res
    }
    throw err
  }
}

class ResponseError extends Error {
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

exports.ResponseError = ResponseError
