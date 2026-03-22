import { PnpmError } from '@pnpm/error'
import { generateQrCode, pollForWebAuthToken, type WebAuthFetchOptions, type WebAuthFetchResponse } from '@pnpm/network.web-auth'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import type { PublishOptions } from 'libnpmpublish'

import { SHARED_CONTEXT } from './utils/shared-context.js'

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
  now: () => number
}

export interface OtpContext {
  Date: OtpDate
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: OtpEnquirer
  fetch: (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>
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

    const fetchOptions: WebAuthFetchOptions = {
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
      const qrCode = generateQrCode(error.body.authUrl)
      globalInfo(`Authenticate your account at:\n${error.body.authUrl}\n\n${qrCode}`)
      otp = await pollForWebAuthToken(error.body.doneUrl, { Date, setTimeout, fetch }, fetchOptions)
    } else {
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
      hint: 'This is unexpected behavior from the registry. Try the command again later and, if the issue persists, verify that your registry supports OTP-based authentication or contact the registry administrator.',
    })
  }
}
