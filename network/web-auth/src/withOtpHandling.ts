import { PnpmError } from '@pnpm/error'

import { generateQrCode } from './generateQrCode.js'
import type { WebAuthFetchOptions, WebAuthFetchResponse } from './pollForWebAuthToken.js'
import { pollForWebAuthToken } from './pollForWebAuthToken.js'

export interface OtpEnquirer {
  prompt: (options: OtpPromptOptions) => Promise<OtpPromptResponse | undefined>
}

export interface OtpPromptOptions {
  message: string
  name: 'otp'
  type: 'input'
}

export interface OtpPromptResponse {
  otp?: string
}

interface OtpDate {
  now: () => number
}

export interface OtpContext {
  Date: OtpDate
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: OtpEnquirer
  fetch: (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
}

export interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface StructuralOtpError {
  code: string
  body?: unknown
}

export const isOtpError = (error: unknown): error is StructuralOtpError =>
  error != null &&
  typeof error === 'object' &&
  'code' in error &&
  error.code === 'EOTP'

/**
 * A validated OTP error with a well-typed body.
 *
 * Use {@link OtpRequiredError.fromUnknown} to validate an unknown EOTP error
 * and produce either a validated `OtpRequiredError` or an `OtpBodyWarning`
 * if the body has unexpected types.
 */
export class OtpRequiredError extends Error {
  readonly code = 'EOTP'
  readonly body: OtpErrorBody
  constructor (body: OtpErrorBody) {
    super('OTP required')
    this.body = body
  }

  /**
   * Validates an unknown EOTP error body and returns either:
   * - an `OtpRequiredError` if the body shape is valid
   * - an `OtpBodyWarning` if the body has fields with unexpected types
   */
  static fromUnknown (error: unknown): OtpRequiredError | OtpBodyWarning {
    const rawBody = error != null && typeof error === 'object' && 'body' in error
      ? error.body as Record<string, unknown> | undefined
      : undefined

    const warnings: string[] = []
    let authUrl: string | undefined
    let doneUrl: string | undefined

    if (rawBody != null && typeof rawBody === 'object') {
      if ('authUrl' in rawBody) {
        if (typeof rawBody.authUrl === 'string') {
          authUrl = rawBody.authUrl
        } else {
          warnings.push(`authUrl has type ${typeof rawBody.authUrl}, expected string`)
        }
      }
      if ('doneUrl' in rawBody) {
        if (typeof rawBody.doneUrl === 'string') {
          doneUrl = rawBody.doneUrl
        } else {
          warnings.push(`doneUrl has type ${typeof rawBody.doneUrl}, expected string`)
        }
      }
    }

    if (warnings.length > 0) {
      return new OtpBodyWarning(warnings, { authUrl, doneUrl })
    }

    return new OtpRequiredError({ authUrl, doneUrl })
  }
}

export class OtpBodyWarning {
  readonly warnings: readonly string[]
  readonly otpError: OtpRequiredError
  constructor (warnings: string[], body: OtpErrorBody) {
    this.warnings = warnings
    this.otpError = new OtpRequiredError(body)
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
 * @throws {@link OtpNonInteractiveError} if OTP is required but the terminal is not interactive.
 * @throws {@link OtpSecondChallengeError} if the registry requests OTP a second time after one was submitted.
 * @throws the original error if OTP handling is not applicable.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/otplease.js for npm's implementation.
 */
export async function withOtpHandling<T> (
  operation: (otp?: string) => Promise<T>,
  context: OtpContext,
  fetchOptions: WebAuthFetchOptions
): Promise<T> {
  const {
    enquirer,
    globalInfo,
    globalWarn,
    process,
  } = context

  try {
    return await operation()
  } catch (error) {
    if (!isOtpError(error)) throw error
    if (!process.stdin.isTTY || !process.stdout.isTTY) {
      throw new OtpNonInteractiveError()
    }

    const validated = OtpRequiredError.fromUnknown(error)
    if (validated instanceof OtpBodyWarning) {
      for (const warning of validated.warnings) {
        globalWarn(`OTP error body: ${warning}`)
      }
    }
    const otpError = validated instanceof OtpBodyWarning ? validated.otpError : validated

    let otp: string | undefined

    if (otpError.body.authUrl && otpError.body.doneUrl) {
      const qrCode = generateQrCode(otpError.body.authUrl)
      globalInfo(`Authenticate your account at:\n${otpError.body.authUrl}\n\n${qrCode}`)
      otp = await pollForWebAuthToken(
        otpError.body.doneUrl,
        context,
        fetchOptions
      )
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
