
import { requestRetryLogger } from '@pnpm/core-loggers'
import { isError, isFetchTimeoutError, redactUrlForDisplay } from '@pnpm/error'
import { operation, type RetryTimeoutOptions } from '@zkochan/retry'
import { type Dispatcher, fetch as undiciFetch, getGlobalDispatcher } from 'undici'

import { holdPermitUntilBodySettles, type NetworkConcurrencyGate } from './networkConcurrencyGate.js'

export { type RetryTimeoutOptions }

interface URLLike {
  href: string
}

// Errors that fail the same way on every attempt: an exhausted local disk,
// a TLS certificate that fails verification, or a CA option with no certificate.
const NO_RETRY_ERROR_CODES = new Set([
  'CERT_CHAIN_TOO_LONG',
  'CERT_HAS_EXPIRED',
  'CERT_NOT_YET_VALID',
  'CERT_REJECTED',
  'CERT_REVOKED',
  'CERT_SIGNATURE_FAILURE',
  'CERT_UNTRUSTED',
  'DEPTH_ZERO_SELF_SIGNED_CERT',
  'ENOSPC',
  'ERR_OSSL_PEM_NO_START_LINE',
  'ERR_PNPM_ENOSPC',
  'ERR_TLS_CERT_ALTNAME_INVALID',
  'HOSTNAME_MISMATCH',
  'INVALID_CA',
  'INVALID_PURPOSE',
  'PATH_LENGTH_EXCEEDED',
  'SELF_SIGNED_CERT_IN_CHAIN',
  'UNABLE_TO_GET_ISSUER_CERT',
  'UNABLE_TO_GET_ISSUER_CERT_LOCALLY',
  'UNABLE_TO_VERIFY_LEAF_SIGNATURE',
])

export function isNonRetryableError (error: unknown): boolean {
  const err = error as { code?: unknown, cause?: { code?: unknown } } | undefined
  return isNonRetryableCode(err?.code) || isNonRetryableCode(err?.cause?.code)
}

function isNonRetryableCode (code: unknown): boolean {
  return typeof code === 'string' && NO_RETRY_ERROR_CODES.has(code)
}

const REDIRECT_CODES = new Set([301, 302, 303, 307, 308])

export function isRedirect (statusCode: number): boolean {
  return REDIRECT_CODES.has(statusCode)
}

export type RequestInfo = string | URLLike | URL

export interface RequestInit extends globalThis.RequestInit {
  retry?: RetryTimeoutOptions
  /**
   * How long the request may make no progress before it fails, in
   * milliseconds. `0` disables it. Bounds the wait for the response head and
   * for each chunk of the body; the connect phase is bounded by the
   * dispatcher's own connect timeout, which cannot be set per request.
   */
  timeout?: number
  dispatcher?: Dispatcher
  /**
   * Shared with every request from one `createFetchFromRegistry` client.
   * Absent for callers that build a request directly.
   */
  concurrencyGate?: NetworkConcurrencyGate
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
    return await attemptWithRetries(url, opts, { maxRetries, op })
  } catch (err) {
    if (err instanceof ResponseError) {
      return err.res
    }
    throw err
  }
}

interface RetryState {
  maxRetries: number
  op: ReturnType<typeof operation>
}

function attemptWithRetries (url: RequestInfo, opts: RequestInit, { maxRetries, op }: RetryState): Promise<Response> {
  return new Promise((resolve, reject) => {
    op.attempt(async (attempt) => {
      const urlString = typeof url === 'string' ? url : url.href ?? url.toString()
      try {
        resolve(await fetchOnce(urlString, opts))
      } catch (error: unknown) {
        handleFailedAttempt(error, { attempt, maxRetries, method: opts.method, op, reject, urlString })
      }
    })
  })
}

async function fetchOnce (urlString: string, { concurrencyGate, ...opts }: RequestInit): Promise<Response> {
  if (concurrencyGate == null) return fetchDirect(urlString, opts)
  await concurrencyGate.acquire()
  let res: Response
  try {
    res = await fetchDirect(urlString, opts)
  } catch (error: unknown) {
    if (isFetchTimeoutError(error)) concurrencyGate.downscaleIfPeersActive()
    concurrencyGate.release()
    throw error
  }
  return holdPermitUntilBodySettles(res, concurrencyGate)
}

async function fetchDirect (urlString: string, opts: RequestInit): Promise<Response> {
  const { retry: _retry, timeout, dispatcher, ...fetchOpts } = opts
  // undici's Response type differs slightly from globalThis.Response (iterator types),
  // requiring the double cast. This is a known TypeScript/undici compatibility issue.
  const res = await undiciFetch(urlString, {
    ...fetchOpts,
    dispatcher: withInactivityTimeout(dispatcher, timeout),
  } as Parameters<typeof undiciFetch>[1]) as unknown as Response
  // A retry on 409 sometimes helps when making requests to the Bit registry.
  if ((res.status >= 500 && res.status < 600) || [408, 409, 420, 429].includes(res.status)) {
    throw new ResponseError(res)
  }
  return res
}

type FetchError = Error & {
  code?: string
  errno?: number
  status?: number
  statusCode?: number
  cause?: { code?: string, errno?: number }
}

interface FailedAttempt extends RetryState {
  attempt: number
  method?: string
  reject: (reason: unknown) => void
  urlString: string
}

function handleFailedAttempt (error: unknown, { attempt, maxRetries, method, op, reject, urlString }: FailedAttempt): void {
  if (isNonRetryableError(error)) {
    // undici's "fetch failed" wrapper hides the TLS reason.
    const cause = (error as { cause?: unknown }).cause
    reject(isError(cause) && isNonRetryableError(cause) ? cause : error)
    return
  }
  // Undici errors may not pass isNativeError check, so we handle them more carefully
  const err = error as FetchError
  const retryTimeout = op.retry(err)
  if (retryTimeout === false) {
    reject(op.mainError())
    return
  }
  const displayUrl = redactUrlForDisplay(urlString)
  requestRetryLogger.debug({
    attempt,
    error: describeErrorForLog(err, { urlString, displayUrl }),
    maxRetries,
    method: method ?? 'GET',
    timeout: retryTimeout,
    url: displayUrl,
  })
}

// Extract error properties into a plain object because Error properties
// are non-enumerable and don't serialize well through the logging system
function describeErrorForLog (err: FetchError, { urlString, displayUrl }: { urlString: string, displayUrl: string }) {
  return {
    name: err.name,
    message: err.message?.replaceAll(urlString, displayUrl),
    code: err.code,
    errno: err.errno,
    // For HTTP errors from ResponseError class
    status: err.status,
    statusCode: err.statusCode,
    // undici wraps the actual network error in a cause property
    cause: err.cause ? {
      code: err.cause.code,
      errno: err.cause.errno,
    } : undefined,
  }
}

/**
 * Hands the timeout to whichever dispatcher serves the request as undici's
 * `headersTimeout` and `bodyTimeout`, which restart on every chunk received.
 * Aborting the request `timeout` after it started instead would kill downloads
 * that are still making progress (https://github.com/pnpm/pnpm/issues/14604).
 */
function withInactivityTimeout (dispatcher: Dispatcher | undefined, timeout: number | undefined): Dispatcher | undefined {
  if (timeout == null) return dispatcher
  return (dispatcher ?? getGlobalDispatcher()).compose((dispatch) => (dispatchOpts, handler) =>
    dispatch({ ...dispatchOpts, headersTimeout: timeout, bodyTimeout: timeout }, handler)
  )
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
