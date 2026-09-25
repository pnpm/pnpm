import { PnpmError } from '@pnpm/error'

import { formatAuthUrlMessage } from './formatAuthUrlMessage.js'
import type { WebAuthFetchOptions, WebAuthFetchResponse } from './pollForWebAuthToken.js'
import { pollForWebAuthToken } from './pollForWebAuthToken.js'
import type { PromptBrowserOpenReadlineInterface } from './promptBrowserOpen.js'
import { promptBrowserOpen } from './promptBrowserOpen.js'

export interface OtpEnquirer {
  input: (options: { message: string }) => Promise<string | undefined>
}

interface OtpDate {
  now: () => number
}

export interface OtpProcess {
  platform?: NodeJS.Platform
  stdin: { isTTY?: boolean }
  stdout: { isTTY?: boolean }
}

export interface OtpContext {
  Date: OtpDate
  setTimeout: (cb: () => void, ms: number) => void
  createReadlineInterface?: () => PromptBrowserOpenReadlineInterface
  enquirer: OtpEnquirer
  fetch: (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: OtpProcess
}

interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface OtpError {
  code: string
  body?: OtpErrorBody
}

export const isOtpError = (error: unknown): error is OtpError =>
  error != null &&
  typeof error === 'object' &&
  'code' in error &&
  error.code === 'EOTP'

export interface OtpHandlingParams<T> {
  context: OtpContext
  fetchOptions: WebAuthFetchOptions
  operation: (otp?: string) => Promise<T>
}

export interface OtpSessionParams {
  context: OtpContext
  fetchOptions: WebAuthFetchOptions
}

export interface OtpSession {
  /**
   * Runs `operation` with the one-time password this session holds, obtaining
   * one on demand.
   *
   * The first operation runs without a password (the caller may still send a
   * configured `--otp`); the password obtained from a challenge is kept and
   * passed to every later operation, so a batch of operations costs one
   * authentication instead of one per operation. When a kept password stops
   * being accepted — a classic OTP expires within a minute — the challenge it
   * triggers obtains a new one and the operation is retried with it.
   */
  run: <T>(operation: (otp?: string) => Promise<T>) => Promise<T>
}

/**
 * Creates an {@link OtpSession}: OTP challenge handling shared across a series
 * of operations.
 *
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry challenges an operation again right after
 *   a freshly obtained one-time password was submitted for it.
 */
export function createOtpSession ({ context, fetchOptions }: OtpSessionParams): OtpSession {
  let sessionOtp: string | undefined
  return {
    async run<T> (operation: (otp?: string) => Promise<T>): Promise<T> {
      let error: unknown
      try {
        return await operation(sessionOtp)
      } catch (err: unknown) {
        if (!isOtpError(err)) throw err
        error = err
      }
      const otp = await resolveOtpChallenge(context, fetchOptions, error as OtpError)
      if (otp == null) throw error
      sessionOtp = otp
      try {
        return await operation(otp)
      } catch (retryError) {
        if (isOtpError(retryError)) {
          throw new OtpSecondChallengeError()
        }
        throw retryError
      }
    },
  }
}

/**
 * Wraps an operation with OTP (one-time password) challenge handling.
 *
 * When the operation throws an error with `code: 'EOTP'`, this function:
 * 1. Uses the web-based authentication flow if the error body contains
 *    `authUrl` and `doneUrl`.
 * 2. Falls back to prompting the user for a classic OTP code.
 * 3. Retries the operation with the obtained OTP.
 *
 * Use {@link createOtpSession} instead when several operations authenticate
 * against the same registry in one run, so they share one one-time password.
 *
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry requests OTP a second time after one was submitted.
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's implementation.
 */
export async function withOtpHandling<T> ({
  context,
  fetchOptions,
  operation,
}: OtpHandlingParams<T>): Promise<T> {
  return createOtpSession({ context, fetchOptions }).run(operation)
}

/**
 * Satisfies an OTP challenge, either through the web-based authentication flow
 * (when the challenge carries both `authUrl` and `doneUrl`) or by prompting for
 * a classic one-time password.
 *
 * @returns the one-time password, or `undefined` when the user supplied none.
 */
async function resolveOtpChallenge (
  context: OtpContext,
  fetchOptions: WebAuthFetchOptions,
  error: OtpError
): Promise<string | undefined> {
  const { enquirer, globalInfo, globalWarn, process } = context
  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new OtpNonInteractiveError(error.body)
  }

  const authUrl = canonicalHttpUrl(error.body?.authUrl)
  const doneUrl = canonicalHttpUrl(error.body?.doneUrl)
  if (authUrl != null && doneUrl != null) {
    globalInfo(formatAuthUrlMessage(authUrl, globalWarn))
    const pollPromise = pollForWebAuthToken({
      context,
      doneUrl,
      fetchOptions,
    })
    return promptBrowserOpen({
      authUrl,
      context,
      pollPromise,
    })
  }

  let otp: string | undefined
  try {
    otp = await enquirer.input({
      message: 'This operation requires a one-time password.\nEnter OTP:',
    })
  } catch (err: unknown) {
    // The user aborted the prompt: re-throw the original challenge.
    if (err instanceof Error && err.name === 'ExitPromptError') return undefined
    throw err
  }
  return otp || undefined
}

/**
 * Synthetic instance of {@link OtpError} meant to be thrown by the callbacks of {@link withOtpHandling}
 * and caught and handled by {@link withOtpHandling}.
 */
export class SyntheticOtpError extends Error implements OtpError {
  readonly code = 'EOTP'
  readonly body?: OtpErrorBody

  constructor (body: OtpErrorBody | undefined) {
    super('This error was meant to be caught by `withOtpHandling`, not to propagate to other parts of the code')
    this.body = body
  }

  /**
   * The challenge a `401` response body carries, or `undefined` when the body
   * is a plain authentication failure. A JSON body with both `authUrl` and
   * `doneUrl` is the web-based flow; a body mentioning `one-time pass` (npm's
   * classic wording) is a classic OTP challenge.
   */
  static fromUnauthorizedBody (body: string): SyntheticOtpError | undefined {
    const parsed = tryParseJson(body)
    if (parsed != null && typeof parsed === 'object' && 'authUrl' in parsed && 'doneUrl' in parsed) {
      return new SyntheticOtpError({
        authUrl: typeof parsed.authUrl === 'string' ? parsed.authUrl : undefined,
        doneUrl: typeof parsed.doneUrl === 'string' ? parsed.doneUrl : undefined,
      })
    }
    if (body.toLowerCase().includes('one-time pass')) {
      return new SyntheticOtpError(undefined)
    }
    return undefined
  }

  static fromUnknownBody (globalWarn: OtpContext['globalWarn'], body: unknown): SyntheticOtpError {
    if (body == null || typeof body !== 'object') {
      return new SyntheticOtpError(undefined)
    }

    let authUrl: string | undefined
    let doneUrl: string | undefined

    if ('authUrl' in body) {
      if (typeof body.authUrl === 'string') {
        authUrl = body.authUrl
      } else {
        globalWarn(`OTP error body: authUrl has type ${typeof body.authUrl}, expected string`)
      }
    }

    if ('doneUrl' in body) {
      if (typeof body.doneUrl === 'string') {
        doneUrl = body.doneUrl
      } else {
        globalWarn(`OTP error body: doneUrl has type ${typeof body.doneUrl}, expected string`)
      }
    }

    return new SyntheticOtpError({ authUrl, doneUrl })
  }
}

function tryParseJson (body: string): unknown {
  try {
    return JSON.parse(body)
  } catch {
    return undefined
  }
}

export class OtpNonInteractiveError extends PnpmError {
  readonly authUrl?: string
  readonly doneUrl?: string

  constructor (body?: OtpErrorBody) {
    super('OTP_NON_INTERACTIVE', 'The registry requires additional authentication, but pnpm is not running in an interactive terminal', {
      hint: 'Re-run this command in an interactive terminal to complete authentication, or provide the --otp option if you are using a classic one-time password (OTP)',
    })
    const authUrl = canonicalHttpUrl(body?.authUrl)
    if (authUrl != null) {
      this.authUrl = authUrl
    }
    const doneUrl = canonicalHttpUrl(body?.doneUrl)
    if (doneUrl != null) {
      this.doneUrl = doneUrl
    }
  }
}

/**
 * Returns the canonical serialization of an `http:`/`https:` URL with any
 * userinfo (`user:pass@`) stripped, or `undefined` for a non-string, an
 * unparsable URL, or any other scheme.
 *
 * These URLs come from the registry and get displayed, opened in a browser,
 * and emitted in parseable error output: the scheme restriction keeps a
 * malicious registry from injecting e.g. a `javascript:` URL into something
 * that opens it, and stripping userinfo keeps credential-shaped data out of
 * logs (the capability tokens automation needs live in the path/query, which
 * are preserved).
 */
export function canonicalHttpUrl (value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined
  try {
    const url = new URL(value)
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined
    url.username = ''
    url.password = ''
    return url.href
  } catch {
    return undefined
  }
}

export class OtpSecondChallengeError extends PnpmError {
  constructor () {
    super('OTP_SECOND_CHALLENGE', 'The registry requested a one-time password (OTP) a second time after one was already provided', {
      hint: 'This is unexpected behavior from the registry. Try the command again later and, if the issue persists, verify that your registry supports OTP-based authentication or contact the registry administrator.',
    })
  }
}
