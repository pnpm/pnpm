import { requestRetryLogger } from '@pnpm/core-loggers'
import { operation, type RetryTimeoutOptions } from '@zkochan/retry'
import { fetch as undiciFetch, type Dispatcher } from 'undici'

export { type RetryTimeoutOptions }

interface URLLike {
  href: string
}

const NO_RETRY_ERROR_CODES = new Set([
  'SELF_SIGNED_CERT_IN_CHAIN',
  'ERR_OSSL_PEM_NO_START_LINE',
])

const REDIRECT_CODES = new Set([301, 302, 303, 307, 308])

export function isRedirect (statusCode: number): boolean {
  return REDIRECT_CODES.has(statusCode)
}

export type RequestInfo = string | URLLike | URL

export interface RequestInit extends globalThis.RequestInit {
  retry?: RetryTimeoutOptions
  timeout?: number
  dispatcher?: Dispatcher
}

export async function fetch (url: RequestInfo, opts: RequestInit = {}): Promise<Response> {
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
    return await new Promise((resolve, reject) => {
      op.attempt(async (attempt) => {
        const urlString = typeof url === 'string' ? url : url.href ?? url.toString()
        const { retry: _retry, timeout, dispatcher, ...fetchOpts } = opts
        const controller = timeout ? new AbortController() : undefined
        const timeoutId = timeout ? setTimeout(() => controller!.abort(), timeout) : undefined
        try {
          // Note: dispatcher is a non-standard option supported by Node.js native fetch (undici)
          // Only include dispatcher if defined, otherwise use the global dispatcher
          const fetchOptions = {
            ...fetchOpts,
            signal: controller?.signal,
            ...(dispatcher ? { dispatcher } : {}),
          } as RequestInit & { dispatcher?: Dispatcher }
          // Use undici's fetch directly to ensure MockAgent integration works in tests
          const res = await undiciFetch(urlString, fetchOptions as Parameters<typeof undiciFetch>[1]) as unknown as Response
          // A retry on 409 sometimes helps when making requests to the Bit registry.
          if ((res.status >= 500 && res.status < 600) || [408, 409, 420, 429].includes(res.status)) {
            throw new ResponseError(res)
          } else {
            resolve(res)
          }
        } catch (error: unknown) {
          // Undici errors may not pass isNativeError check, so we handle them more carefully
          const err = error as Error & { code?: string, cause?: { code?: string } }
          // Check error code in both error.code and error.cause.code (undici wraps errors)
          const errorCode = err?.code ?? err?.cause?.code
          if (
            typeof errorCode === 'string' &&
            NO_RETRY_ERROR_CODES.has(errorCode)
          ) {
            throw error
          }
          const retryTimeout = op.retry(err)
          if (retryTimeout === false) {
            reject(op.mainError())
            return
          }
          // Extract error properties into a plain object because Error properties
          // are non-enumerable and don't serialize well through the logging system
          const errorInfo = {
            name: err.name,
            message: err.message,
            code: err.code,
            errno: (err as Error & { errno?: number }).errno,
            // For HTTP errors from ResponseError class
            status: (err as Error & { status?: number }).status,
            statusCode: (err as Error & { statusCode?: number }).statusCode,
            // undici wraps the actual network error in a cause property
            cause: err.cause ? {
              code: err.cause.code,
              errno: (err.cause as { errno?: number }).errno,
            } : undefined,
          }
          requestRetryLogger.debug({
            attempt,
            error: errorInfo,
            maxRetries,
            method: opts.method ?? 'GET',
            timeout: retryTimeout,
            url: url.toString(),
          })
        } finally {
          if (timeoutId) clearTimeout(timeoutId)
        }
      })
    })
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
