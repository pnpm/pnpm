import { PnpmError } from '@pnpm/error'

import { generateQrCode } from './generateQrCode.js'
import type { WebAuthFetchOptions, WebAuthFetchResponse } from './pollForWebAuthToken.js'
import { pollForWebAuthToken } from './pollForWebAuthToken.js'

export interface OtpHandlingEnquirer {
  prompt: (options: OtpHandlingPromptOptions) => Promise<OtpHandlingPromptResponse | undefined>
}

export interface OtpHandlingPromptOptions {
  message: string
  name: 'otp'
  type: 'input'
}

export interface OtpHandlingPromptResponse {
  otp?: string
}

export interface OtpHandlingContext {
  Date: { now: () => number }
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: OtpHandlingEnquirer
  fetch: (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>
  globalInfo: (message: string) => void
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
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

/**
 * Wraps an operation with OTP (one-time password) challenge handling.
 *
 * When the operation throws an error with `code: 'EOTP'`, this function:
 * 1. Uses the web-based authentication flow if the error body contains
 *    `authUrl` and `doneUrl`.
 * 2. Falls back to prompting the user for a classic OTP code.
 * 3. Retries the operation with the obtained OTP.
 *
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry requests OTP a second time after one was submitted.
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's implementation.
 */
export async function withOtpHandling<T> (
  operation: (otp?: string) => Promise<T>,
  context: OtpHandlingContext,
  fetchOptions: WebAuthFetchOptions
): Promise<T> {
  try {
    return await operation()
  } catch (error) {
    if (!isOtpError(error)) throw error
    if (!context.process.stdin.isTTY || !context.process.stdout.isTTY) {
      throw new OtpNonInteractiveError()
    }

    let otp: string | undefined

    if (error.body?.authUrl && error.body?.doneUrl) {
      const qrCode = generateQrCode(error.body.authUrl)
      context.globalInfo(`Authenticate your account at:\n${error.body.authUrl}\n\n${qrCode}`)
      otp = await pollForWebAuthToken(
        error.body.doneUrl,
        { Date: context.Date, setTimeout: context.setTimeout, fetch: context.fetch },
        fetchOptions
      )
    } else {
      const enquirerResponse = await context.enquirer.prompt({
        message: 'This operation requires a one-time password.\nEnter OTP:',
        name: 'otp',
        type: 'input',
      })

      // Use || (not ??) so that empty-string input is treated as "no OTP provided"
      otp = enquirerResponse?.otp || undefined
    }

    if (otp != null) {
      try {
        return await operation(otp)
      } catch (retryError) {
        if (isOtpError(retryError)) {
          throw new OtpSecondChallengeError()
        }

        throw retryError
      }
    }

    throw error
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
      hint: 'This is unexpected behavior from the registry. Try the command again later and, if the issue persists, verify that your registry supports OTP-based authentication or contact the registry administrator.',
    })
  }
}
