import { PnpmError } from '@pnpm/error'

import { generateQrCode } from './generateQrCode.js'
import type { WebAuthFetchOptions, WebAuthFetchResponse } from './pollForWebAuthToken.js'
import { pollForWebAuthToken } from './pollForWebAuthToken.js'
import type { PromptBrowserOpenReadlineInterface } from './promptBrowserOpen.js'
import { promptBrowserOpen } from './promptBrowserOpen.js'

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
export async function withOtpHandling<T> ({
  context,
  fetchOptions,
  operation,
}: OtpHandlingParams<T>): Promise<T> {
  const {
    enquirer,
    globalInfo,
    process,
  } = context

  try {
    return await operation()
  } catch (error) {
    if (!isOtpError(error)) throw error
    if (!process.stdin.isTTY || !process.stdout.isTTY) {
      throw new OtpNonInteractiveError()
    }

    let otp: string | undefined

    if (error.body?.authUrl && error.body?.doneUrl) {
      const qrCode = generateQrCode(error.body.authUrl)
      globalInfo(`Authenticate your account at:\n${error.body.authUrl}\n\n${qrCode}`)
      const pollPromise = pollForWebAuthToken({
        context,
        doneUrl: error.body.doneUrl,
        fetchOptions,
      })
      otp = await promptBrowserOpen({
        authUrl: error.body.authUrl,
        context,
        pollPromise,
      })
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
