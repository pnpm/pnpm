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
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
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
  'npm-notice'?: string[]
  'www-authenticate'?: string[]
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

/**
 * Publish a package, handling OTP challenges (classic OTP prompt and webauth browser flow).
 *
 * @throws {@link OtpWebAuthTimeoutError} if the webauth browser flow times out.
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry requests OTP a second time after one was submitted.
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's implementation.
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
      otp = await webAuthOtp(error.body.authUrl, error.body.doneUrl, context, fetchOptions)
    } else {
      const npmNotice = error.headers?.['npm-notice']
      const npmNoticeAuthUrl = npmNotice && extractUrlFromNpmNotice(npmNotice)
      const registry = publishOptions.registry
      const npmNoticeDoneUrl = npmNoticeAuthUrl && registry
        ? derivePollUrl(registry, npmNoticeAuthUrl)
        : undefined

      if (npmNoticeAuthUrl && npmNoticeDoneUrl) {
        const noticeMessage = npmNotice!.join('\n')
        otp = await webAuthOtp(npmNoticeAuthUrl, npmNoticeDoneUrl, context, fetchOptions, noticeMessage)
      } else {
        const enquirerResponse = await context.enquirer.prompt({
          message: 'This operation requires a one-time password.\nEnter OTP:',
          name: 'otp',
          type: 'input',
        })
        otp = enquirerResponse?.otp || undefined
      }
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

// Matches a URL in an npm-notice message such as:
// "Open https://www.npmjs.com/login/TOKEN to use your security key for authentication"
const NPM_NOTICE_URL_REGEX = /https?:\/\/[^\s<>"{}|\\^`[\]]+/

function extractUrlFromNpmNotice (npmNotice: string[]): string | undefined {
  for (const notice of npmNotice) {
    const match = NPM_NOTICE_URL_REGEX.exec(notice)
    if (match) return match[0]
  }
  return undefined
}

function derivePollUrl (registry: string, authUrl: string): string | undefined {
  // Extracts the login token from URLs like https://www.npmjs.com/login/TOKEN
  // and constructs the registry poll endpoint at {registry}/-/v1/login/poll/TOKEN
  const tokenMatch = /\/login\/([^/\s]+)/.exec(authUrl)
  if (!tokenMatch) return undefined
  const token = tokenMatch[1]
  return `${registry}-/v1/login/poll/${token}`
}

async function webAuthOtp (authUrl: string, doneUrl: string, context: OtpContext, fetchOptions: OtpWebAuthFetchOptions, message?: string): Promise<string> {
  const qrCode = await generateQrCode(authUrl)
  context.globalInfo(`${message ?? `Authenticate your account at:\n${authUrl}`}\n\n${qrCode}`)
  const startTime = context.Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  while (true) {
    if (context.Date.now() - startTime > timeout) {
      throw new OtpWebAuthTimeoutError(context.Date.now(), startTime, timeout)
    }
    // eslint-disable-next-line no-await-in-loop
    await new Promise<void>(resolve => context.setTimeout(resolve, 1000))
    let response: OtpWebAuthFetchResponse
    try {
      // eslint-disable-next-line no-await-in-loop
      response = await context.fetch(doneUrl, fetchOptions)
    } catch {
      continue
    }
    if (!response.ok) continue
    let body: { done?: boolean; token?: string }
    try {
      // eslint-disable-next-line no-await-in-loop
      body = await response.json() as { done?: boolean; token?: string }
    } catch {
      continue
    }
    if (body.done && body.token) {
      return body.token
    }
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
