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

export interface OtpWebAuthFetchResponse {
  readonly headers: {
    get: (name: string) => string | null
  }
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

const URL_IN_NOTICE_RE = /https?:\/\/\S+/i

/**
 * Extract a URL from an npm-notice header message.
 *
 * For example, given:
 *   "Open https://www.npmjs.com/login/abc-123 to use your security key for authentication"
 * Returns:
 *   "https://www.npmjs.com/login/abc-123"
 */
export function extractUrlFromNotice (notice: string): string | undefined {
  return URL_IN_NOTICE_RE.exec(notice)?.[0]
}

/**
 * Publish a package, handling OTP challenges:
 * - Web auth flow (authUrl/doneUrl in error body with doneUrl polling)
 * - npm-notice flow (URL in npm-notice header with QR code display, then OTP prompt)
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
  context = SHARED_CONTEXT,
  manifest,
  publishOptions,
  tarballData,
}: OtpParams): Promise<OtpPublishResponse> {
  let response: OtpPublishResponse
  try {
    response = await context.publish(manifest, tarballData, publishOptions)
  } catch (error) {
    if (!isOtpError(error)) throw error
    if (!context.process.stdin.isTTY || !context.process.stdout.isTTY) {
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
      // Web auth flow: display authUrl with QR code, poll doneUrl for token
      otp = await webAuthOtp(error.body.authUrl, error.body.doneUrl, context, fetchOptions)
    } else {
      // Display npm-notice URL with QR code if available
      await displayNpmNotice(error, context)
      // Prompt for manual OTP entry
      const enquirerResponse = await context.enquirer.prompt({
        message: 'This operation requires a one-time password.\nEnter OTP:',
        name: 'otp',
        type: 'input',
      })
      otp = enquirerResponse?.otp || undefined
    }
    if (otp != null) {
      try {
        return await context.publish(manifest, tarballData, { ...publishOptions, otp })
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
 * If the OTP error contains npm-notice headers with a URL, display the
 * notice messages and a QR code for the URL.
 */
async function displayNpmNotice (error: OtpError, context: OtpContext): Promise<void> {
  const notices = error.headers?.['npm-notice']
  if (!notices?.length) return

  for (const notice of notices) {
    context.globalInfo(notice)
    const url = extractUrlFromNotice(notice)
    if (url) {
      // eslint-disable-next-line no-await-in-loop
      const qrCode = await generateQrCode(url)
      context.globalInfo(`\n${qrCode}`)
    }
  }
}

async function webAuthOtp (authUrl: string, doneUrl: string, context: OtpContext, fetchOptions: OtpWebAuthFetchOptions): Promise<string> {
  const qrCode = await generateQrCode(authUrl)
  context.globalInfo(`Authenticate your account at:\n${authUrl}\n\n${qrCode}`)
  const startTime = context.Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  const pollIntervalMs = 1000

  while (true) {
    if (context.Date.now() - startTime > timeout) {
      throw new OtpWebAuthTimeoutError(context.Date.now(), startTime, timeout)
    }
    // eslint-disable-next-line no-await-in-loop
    await new Promise<void>(resolve => context.setTimeout(resolve, pollIntervalMs))
    let response: OtpWebAuthFetchResponse
    try {
      // eslint-disable-next-line no-await-in-loop
      response = await context.fetch(doneUrl, fetchOptions)
    } catch {
      continue
    }

    if (response.status === 202) {
      // Registry is still waiting for authentication.
      // Respect Retry-After header if present by waiting the additional time
      // beyond the default poll interval already elapsed above.
      const retryAfterSeconds = Number(response.headers.get('retry-after'))
      if (Number.isFinite(retryAfterSeconds)) {
        const additionalMs = retryAfterSeconds * 1000 - pollIntervalMs
        if (additionalMs > 0) {
          // eslint-disable-next-line no-await-in-loop
          await new Promise<void>(resolve => context.setTimeout(resolve, additionalMs))
        }
      }
      continue
    }

    if (response.ok) {
      // Treat any 2xx (other than 202) the same: try to extract a token from the body.
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
    // Non-ok status (4xx, 5xx, etc.): retry after the default interval
  }
}

function generateQrCode (url: string): Promise<string> {
  return new Promise(resolve => {
    qrcodeTerminal.generate(url, { small: true }, resolve)
  })
}

export class OtpWebAuthTimeoutError extends PnpmError {
  readonly endTime: number
  readonly startTime: number
  readonly timeout: number
  constructor (endTime: number, startTime: number, timeout: number) {
    super('WEBAUTH_TIMEOUT', 'Web authentication timed out. Please try again.')
    this.endTime = endTime
    this.startTime = startTime
    this.timeout = timeout
  }
}

export class OtpNonInteractiveError extends PnpmError {
  constructor () {
    super('OTP_NON_INTERACTIVE', 'The registry requires a one-time password (OTP) but pnpm is not running in an interactive terminal. Please set the --otp option.')
  }
}

export class OtpSecondChallengeError extends PnpmError {
  constructor () {
    super('OTP_SECOND_CHALLENGE', 'The registry requested a one-time password (OTP) a second time after one was already provided. This is unexpected behavior from the registry.')
  }
}
