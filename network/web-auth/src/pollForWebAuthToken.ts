import { WebAuthTimeoutError } from './WebAuthTimeoutError.js'

export interface WebAuthFetchOptions {
  method: 'GET'
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface WebAuthFetchResponseHeaders {
  get: (name: string) => string | null
}

export interface WebAuthFetchResponse {
  readonly headers: WebAuthFetchResponseHeaders
  readonly json: () => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface WebAuthContext {
  Date: { now: () => number }
  setTimeout: (cb: () => void, ms: number) => void
  fetch: (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>
}

export interface PollForWebAuthTokenParams {
  context: WebAuthContext
  doneUrl: string
  fetchOptions: WebAuthFetchOptions
  timeoutMs?: number
}

/**
 * Polls a registry's "done" URL until an authentication token is returned.
 *
 * The caller is responsible for displaying the authentication URL (and optional
 * QR code) to the user before calling this function.
 *
 * @returns The authentication token string.
 *
 * @throws {@link WebAuthTimeoutError} if the timeout is exceeded.
 */
export async function pollForWebAuthToken ({
  context: { Date, fetch, setTimeout },
  doneUrl,
  fetchOptions,
  timeoutMs = 5 * 60 * 1000,
}: PollForWebAuthTokenParams): Promise<string> {
  const startTime = Date.now()
  const pollIntervalMs = 1000

  while (true) {
    const now = Date.now()
    if (now - startTime > timeoutMs) {
      throw new WebAuthTimeoutError(now, startTime, timeoutMs)
    }
    // eslint-disable-next-line no-await-in-loop
    await new Promise<void>(resolve => setTimeout(resolve, pollIntervalMs))
    let response: WebAuthFetchResponse
    try {
      // eslint-disable-next-line no-await-in-loop
      response = await fetch(doneUrl, fetchOptions)
    } catch {
      continue
    }

    if (!response.ok) continue

    if (response.status === 202) {
      // Registry is still waiting for authentication.
      // Respect Retry-After header if present by waiting the additional time
      // beyond the default poll interval already elapsed above, but do not
      // exceed the overall timeout.
      const retryAfterSeconds = Number(response.headers.get('retry-after'))
      if (Number.isFinite(retryAfterSeconds)) {
        const additionalMs = retryAfterSeconds * 1000 - pollIntervalMs
        if (additionalMs > 0) {
          const nowAfterPoll = Date.now()
          const remainingMs = timeoutMs - (nowAfterPoll - startTime)
          if (remainingMs <= 0) {
            throw new WebAuthTimeoutError(nowAfterPoll, startTime, timeoutMs)
          }
          const sleepMs = Math.min(additionalMs, remainingMs)
          // eslint-disable-next-line no-await-in-loop
          await new Promise<void>(resolve => setTimeout(resolve, sleepMs))
        }
      }
      continue
    }

    let body: { token?: string }
    try {
      // eslint-disable-next-line no-await-in-loop
      body = await response.json() as { token?: string }
    } catch {
      continue
    }
    if (body.token) {
      return body.token
    }
  }
}
