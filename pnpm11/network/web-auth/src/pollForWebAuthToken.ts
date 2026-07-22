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

export interface WebAuthFetchResponseBodyReader {
  read: () => Promise<{ done: boolean, value?: Uint8Array }>
  cancel: () => Promise<void>
}

export interface WebAuthFetchResponseBody {
  cancel: () => Promise<void>
  getReader: () => WebAuthFetchResponseBodyReader
}

export interface WebAuthFetchResponse {
  readonly body?: WebAuthFetchResponseBody | null
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
 * The most bytes of a poll response body that are read. The expected body is
 * a small JSON object carrying the token, and the URL it comes from is
 * registry-controlled, so an unbounded read on every poll tick would let a
 * malicious or compromised registry grow memory at will.
 */
const TOKEN_BODY_LIMIT = 64 * 1024

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

    if (!response.ok) {
      discardBody(response)
      continue
    }

    if (response.status === 202) {
      discardBody(response)
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

    // eslint-disable-next-line no-await-in-loop
    const body = await readTokenBody(response) as { token?: string } | undefined
    if (body?.token) {
      return body.token
    }
  }
}

/**
 * Reads and parses a poll response body, applying {@link TOKEN_BODY_LIMIT}
 * when the raw stream is exposed. Returns `undefined` — which the poll loop
 * treats the same as an unparsable body, retrying on the next tick — for an
 * oversized or truncated body.
 */
async function readTokenBody (response: WebAuthFetchResponse): Promise<unknown> {
  // A response that exposes no readable body stream can only be read whole via
  // json(), which the size cap cannot bound. The production undici-backed fetch
  // always exposes `body` as a ReadableStream, so this uncapped path is reached
  // only by stream-less WebAuthFetch stand-ins (the json()-based test mocks);
  // every real transport goes through the capped stream read below.
  if (response.body === undefined) {
    try {
      return await response.json()
    } catch {
      return undefined
    }
  }
  const contentLength = Number(response.headers.get('content-length'))
  if (Number.isFinite(contentLength) && contentLength > TOKEN_BODY_LIMIT) {
    discardBody(response)
    return undefined
  }
  if (response.body === null) return undefined
  let reader: WebAuthFetchResponseBodyReader
  try {
    reader = response.body.getReader()
  } catch {
    return undefined
  }
  const chunks: Uint8Array[] = []
  let total = 0
  try {
    while (true) {
      // eslint-disable-next-line no-await-in-loop
      const { done, value } = await reader.read()
      if (done) break
      if (value == null) continue
      total += value.length
      if (total > TOKEN_BODY_LIMIT) {
        reader.cancel().catch(() => {})
        return undefined
      }
      chunks.push(value)
    }
  } catch {
    return undefined
  }
  try {
    return JSON.parse(new TextDecoder().decode(Buffer.concat(chunks)))
  } catch {
    return undefined
  }
}

/**
 * Cancels a response body the poll loop will not read (non-ok and 202
 * responses), so its payload is not transferred on every poll tick.
 */
function discardBody (response: WebAuthFetchResponse): void {
  try {
    response.body?.cancel().catch(() => {})
  } catch {
    // Cancellation is best-effort: a body that cannot be cancelled is simply
    // left unread.
  }
}
