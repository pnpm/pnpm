import { PnpmError } from '@pnpm/error'
import { fetch as pnpmFetch } from '@pnpm/fetch'
import { globalInfo as logGlobalInfo } from '@pnpm/logger'
import enquirer from 'enquirer'
import { type PublishOptions, publish } from 'libnpmpublish'

// ---- Response type definitions ----

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

export type OtpPublishFn = (
  manifest: object,
  tarballData: Buffer,
  options: PublishOptions
) => Promise<OtpPublishResponse>

export interface OtpDate {
  now: (this: this) => number
}

// ---- Context interface ----

export interface OtpContext {
  Date: OtpDate
  delay: (ms: number) => Promise<void>
  fetch: (url: string) => Promise<OtpWebAuthFetchResponse>
  globalInfo: (message: string) => void
  isInteractive: boolean
  prompt: () => Promise<string | undefined>
  publish: OtpPublishFn
}

// ---- Params interface ----

export interface OtpParams {
  context?: OtpContext
  manifest: object
  publishOptions: PublishOptions
  tarballData: Buffer
}

// ---- Shared context (real implementations) ----

async function sharedPrompt (): Promise<string | undefined> {
  const { otp } = await enquirer.prompt<{ otp: string }>({
    message: 'This operation requires a one-time password.\nEnter OTP:',
    name: 'otp',
    type: 'input',
  })
  return otp || undefined
}

export const SHARED_CONTEXT: OtpContext = {
  Date,
  delay: (ms) => new Promise<void>(resolve => setTimeout(resolve, ms)),
  fetch: (url) => pnpmFetch(url),
  globalInfo: logGlobalInfo,
  get isInteractive () { return !!(process.stdin.isTTY && process.stdout.isTTY) },
  prompt: sharedPrompt,
  publish: publish as unknown as OtpPublishFn,
}

// ---- Internal helpers ----

interface OtpErrorBody {
  authUrl?: string
  doneUrl?: string
}

interface OtpError {
  code: string
  body?: OtpErrorBody
}

function isOtpError (error: unknown): error is OtpError {
  return (
    error != null &&
    typeof error === 'object' &&
    'code' in error &&
    (error as Record<string, unknown>).code === 'EOTP'
  )
}

// ---- Main function ----

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
  publishOptions,
  tarballData,
}: OtpParams): Promise<OtpPublishResponse> {
  let response: OtpPublishResponse
  try {
    response = await context.publish(manifest, tarballData, publishOptions)
  } catch (error) {
    if (context.isInteractive && isOtpError(error)) {
      let otp: string | undefined
      if (error.body?.authUrl && error.body?.doneUrl) {
        otp = await webAuthOtp(error.body.authUrl, error.body.doneUrl, context)
      } else {
        otp = await context.prompt()
      }
      if (otp != null) {
        return publishWithOtpHandling({
          context,
          manifest,
          tarballData,
          publishOptions: { ...publishOptions, otp },
        })
      }
    }
    throw error
  }
  return response
}

// ---- Webauth helpers ----

async function webAuthOtp (authUrl: string, doneUrl: string, context: OtpContext): Promise<string> {
  context.globalInfo(`Authenticate your account at:\n${authUrl}`)
  return pollWebAuthDone(doneUrl, context)
}

async function pollWebAuthDone (doneUrl: string, context: OtpContext): Promise<string> {
  const startTime = context.Date.now()
  const timeout = 5 * 60 * 1000 // 5 minutes

  while (true) {
    if (context.Date.now() - startTime > timeout) {
      throw new OtpWebAuthTimeoutError()
    }
    // eslint-disable-next-line no-await-in-loop
    await context.delay(1000)
    let response: OtpWebAuthFetchResponse
    try {
      // eslint-disable-next-line no-await-in-loop
      response = await context.fetch(doneUrl)
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

// ---- Error classes ----

export class OtpWebAuthTimeoutError extends PnpmError {
  constructor () {
    super('WEBAUTH_TIMEOUT', 'Web authentication timed out. Please try again.')
  }
}
