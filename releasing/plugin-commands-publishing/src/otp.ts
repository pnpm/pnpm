import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { type PublishOptions } from 'libnpmpublish'
import qrcodeTerminal from 'qrcode-terminal'
import { SHARED_CONTEXT } from './utils/shared-context.js'

export interface OtpWebAuthFetchOptions {
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

export interface OtpWebAuthFetchResponseHeaders {
  get: (this: this, name: 'retry-after') => string | null
}

export interface OtpWebAuthFetchResponse {
  readonly headers: OtpWebAuthFetchResponseHeaders
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface OtpPublishResponse {
  readonly ok: boolean
  readonly status: number
  readonly statusText: string
  readonly text: () => Promise<string>
}

export interface OtpEnquirer {
  prompt: (this: this, options: OtpEnquirerOptions) => Promise<OtpEnquirerResponse | undefined>
}

export interface OtpEnquirerOptions {
  message: string
  name: 'otp'
  type: 'input'
}

export interface OtpEnquirerResponse {
  otp?: string
}

export type OtpPublishFn = (
  manifest: ExportedManifest,
  tarballData: Buffer,
  options: PublishOptions
) => Promise<OtpPublishResponse>

export interface OtpDate {
  now: (this: this) => number
}

export interface OtpContext {
  Date: OtpDate
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: OtpEnquirer
  fetch: (url: string, options: OtpWebAuthFetchOptions) => Promise<OtpWebAuthFetchResponse>
  globalInfo: (message: string) => void
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
  publish: OtpPublishFn
}

export interface OtpParams {
  context?: OtpContext
  manifest: ExportedManifest
  publishOptions: PublishOptions
  tarballData: Buffer
}

export { SHARED_CONTEXT }

interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface OtpErrorHeaders {
  'www-authenticate'?: string[]
  'npm-notice'?: string[]
}

interface OtpError {
  code: string
  body?: OtpErrorBody
  headers?: OtpErrorHeaders
}

const isOtpError = (error: unknown): error is OtpError =>
  error != null &&
  typeof error === 'object' &&
  'code' in error &&
  error.code === 'EOTP'

const URL_IN_STRING_RE = /https?:\/\/\S+/gi

/**
 * Extract all URLs from a string.
 *
 * For example, given:
 *   "Open https://www.npmjs.com/login/abc-123 or visit https://example.com for help"
 * Yields:
 *   "https://www.npmjs.com/login/abc-123"
 *   "https://example.com"
 */
export function * extractUrlsFromString (text: string): Generator<string> {
  for (const match of text.matchAll(URL_IN_STRING_RE)) {
    yield match[0]
  }
}

/**
 * Publish a package, handling OTP challenges:
 * - Web based authentication flow (authUrl/doneUrl in error body with doneUrl polling)
 * - Classic OTP prompt (manual code entry)
 *
 * @throws {@link OtpWebAuthTimeoutError} if the webauth browser flow times out.
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry requests OTP a second time after one was submitted.
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's implementation.
 * @see https://github.com/npm/npm-profile/blob/main/lib/index.js for the webauth polling flow.
 */
export async function publishWithOtpHandling ({
  context: {
    Date,
    setTimeout,
    enquirer,
    fetch,
    globalInfo,
    process,
    publish,
  } = SHARED_CONTEXT,
  manifest,
  publishOptions,
  tarballData,
}: OtpParams): Promise<OtpPublishResponse> {
  let response: OtpPublishResponse

  try {
    response = await publish(manifest, tarballData, publishOptions)
  } catch (error) {
    if (!isOtpError(error)) throw error
    if (!process.stdin.isTTY || !process.stdout.isTTY) {
      throw new OtpNonInteractiveError()
    }

    const fetchOptions: OtpWebAuthFetchOptions = {
      method: 'GET',
      retry: {
        factor: publishOptions.fetchRetryFactor,
        maxTimeout: publishOptions.fetchRetryMaxtimeout,
        minTimeout: publishOptions.fetchRetryMintimeout,
        retries: publishOptions.fetchRetries,
      },
      timeout: publishOptions.timeout,
    }

    let otp: string | undefined

    if (error.body?.authUrl && error.body?.doneUrl) {
      otp = await webAuthOtp(error.body.authUrl, error.body.doneUrl, { Date, setTimeout, fetch, globalInfo }, fetchOptions)
    } else {
      // NOTE: I worry that this notice may mislead the user,
      //       I will wait for @zkochan to test the OTP again
      //       before deciding whether to delete or keep this
      //       line.
      displayNpmNotice(error, globalInfo)

      const enquirerResponse = await enquirer.prompt({
        message: 'This operation requires a one-time password.\nEnter OTP:',
        name: 'otp',
        type: 'input',
      })

      // Use || (not ??) so that empty-string input is treated as "no OTP provided"
      otp = enquirerResponse?.otp || undefined
    }

    if (otp != null) {
      try {
        return await publish(manifest, tarballData, { ...publishOptions, otp })
      } catch (retryError) {
        if (isOtpError(retryError)) {
          throw new OtpSecondChallengeError()
        }

        throw retryError
      }
    }

    throw error
  }

  return response
}

/**
 * If the OTP error contains npm-notice headers with URLs, display the
 * notice messages and a QR code for each URL.
 */
function displayNpmNotice (error: OtpError, globalInfo: OtpContext['globalInfo']): void {
  const notices = error.headers?.['npm-notice']
  if (!notices?.length) return

  for (const notice of notices) {
    globalInfo(notice)
    for (const url of extractUrlsFromString(notice)) {
      const qrCode = generateQrCode(url)
      globalInfo(`\n${qrCode}\n`)
    }
  }
}

async function webAuthOtp (
  authUrl: string,
  doneUrl: string,
  { Date, setTimeout, fetch, globalInfo }: Pick<OtpContext, 'Date' | 'setTimeout' | 'fetch' | 'globalInfo'>,
  fetchOptions: OtpWebAuthFetchOptions
): Promise<string> {
  const qrCode = generateQrCode(authUrl)
  globalInfo(`Authenticate your account at:\n${authUrl}\n\n${qrCode}`)
  const startTime = Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  const pollIntervalMs = 1000

  while (true) {
    const now = Date.now()
    if (now - startTime > timeout) {
      throw new OtpWebAuthTimeoutError(now, startTime, timeout)
    }
    // eslint-disable-next-line no-await-in-loop
    await new Promise<void>(resolve => setTimeout(resolve, pollIntervalMs))
    let response: OtpWebAuthFetchResponse
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
          const remainingMs = timeout - (nowAfterPoll - startTime)
          if (remainingMs <= 0) {
            throw new OtpWebAuthTimeoutError(nowAfterPoll, startTime, timeout)
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

function generateQrCode (text: string): string {
  let qrCode: string | undefined
  qrcodeTerminal.generate(text, { small: true }, (code: string) => {
    qrCode = code
  })
  if (qrCode != null) return qrCode
  /* istanbul ignore next */
  throw new Error('we were expecting qrcode-terminal to be fully synchronous, but it fails to execute the callback')
}

export class OtpWebAuthTimeoutError extends PnpmError {
  readonly endTime: number
  readonly startTime: number
  readonly timeout: number
  constructor (endTime: number, startTime: number, timeout: number) {
    super('WEBAUTH_TIMEOUT', 'Web based authentication timed out', {
      hint: 'Try again, do not take too long this time',
    })
    this.endTime = endTime
    this.startTime = startTime
    this.timeout = timeout
  }
}

export class OtpNonInteractiveError extends PnpmError {
  constructor () {
    super('OTP_NON_INTERACTIVE', 'The registry requires additional authentication, but pnpm is not running in an interactive terminal', {
      hint: 'Re-run this command in an interactive terminal to complete authentication, or provide the --otp option if you are using a classic one-time password (OTP)',
    })
  }
}

export class OtpSecondChallengeError extends PnpmError {
  constructor () {
    super('OTP_SECOND_CHALLENGE', 'The registry requested a one-time password (OTP) a second time after one was already provided', {
      hint: 'This is unexpected behavior from the registry, you can do nothing about it, sorry',
    })
  }
}
