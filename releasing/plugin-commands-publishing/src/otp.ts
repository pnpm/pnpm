import { PnpmError } from '@pnpm/error'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { fetch } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import enquirer from 'enquirer'
import { type PublishOptions, publish } from 'libnpmpublish'

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

// @types/libnpmpublish unfortunately uses an outdated type definition of package.json
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
  delay: (ms: number) => Promise<void>
  enquirer: OtpEnquirer
  fetch: (url: string, options: OtpWebAuthFetchOptions) => Promise<OtpWebAuthFetchResponse>
  globalInfo: (message: string) => void
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
  publish: OtpPublishFn
}

export interface OtpParams {
  context?: OtpContext
  manifest: ExportedManifest
  otpRetryAllowed?: boolean
  publishOptions: PublishOptions
  tarballData: Buffer
}

export const SHARED_CONTEXT: OtpContext = {
  Date,
  delay: (ms) => new Promise<void>(resolve => setTimeout(resolve, ms)),
  enquirer: enquirer as unknown as OtpEnquirer,
  fetch: (url, options) => fetch(url, options),
  globalInfo,
  process,
  publish: publish as unknown as OtpPublishFn,
}

interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface OtpError {
  code: string
  body?: OtpErrorBody
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
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's `otplease()` implementation.
 */
export async function publishWithOtpHandling ({
  context = SHARED_CONTEXT,
  manifest,
  otpRetryAllowed = true,
  publishOptions,
  tarballData,
}: OtpParams): Promise<OtpPublishResponse> {
  let response: OtpPublishResponse
  try {
    response = await context.publish(manifest, tarballData, publishOptions)
  } catch (error) {
    if (otpRetryAllowed && !!(context.process.stdin.isTTY && context.process.stdout.isTTY) && isOtpError(error)) {
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
        const enquirerResponse = await context.enquirer.prompt({
          message: 'This operation requires a one-time password.\nEnter OTP:',
          name: 'otp',
          type: 'input',
        })
        otp = enquirerResponse?.otp || undefined
      }
      if (otp != null) {
        return publishWithOtpHandling({
          context,
          manifest,
          otpRetryAllowed: false,
          tarballData,
          publishOptions: { ...publishOptions, otp },
        })
      }
    }
    throw error
  }
  return response
}

async function webAuthOtp (authUrl: string, doneUrl: string, context: OtpContext, fetchOptions: OtpWebAuthFetchOptions): Promise<string> {
  context.globalInfo(`Authenticate your account at:\n${authUrl}`)
  const startTime = context.Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  while (true) {
    if (context.Date.now() - startTime > timeout) {
      throw new OtpWebAuthTimeoutError(context.Date.now(), startTime, timeout)
    }
    // eslint-disable-next-line no-await-in-loop
    await context.delay(1000)
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
