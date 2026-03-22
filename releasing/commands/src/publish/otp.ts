import {
  type OtpHandlingContext,
  type OtpHandlingEnquirer,
  type WebAuthFetchOptions,
  type WebAuthFetchResponse,
  withOtpHandling,
} from '@pnpm/network.web-auth'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import type { PublishOptions } from 'libnpmpublish'

import { SHARED_CONTEXT } from './utils/shared-context.js'

export interface OtpPublishResponse {
  readonly ok: boolean
  readonly status: number
  readonly statusText: string
  readonly text: () => Promise<string>
}

export type OtpPublishFn = (
  manifest: ExportedManifest,
  tarballData: Buffer,
  options: PublishOptions
) => Promise<OtpPublishResponse>

export interface OtpContext {
  Date: { now: () => number }
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: OtpHandlingEnquirer
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

/**
 * Publish a package, handling OTP challenges:
 * - Web based authentication flow (authUrl/doneUrl in error body with doneUrl polling)
 * - Classic OTP prompt (manual code entry)
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

  const otpContext: OtpHandlingContext = { Date, setTimeout, enquirer, fetch, globalInfo, process }

  return withOtpHandling(
    (otp) => publish(manifest, tarballData, otp != null ? { ...publishOptions, otp } : publishOptions),
    otpContext,
    fetchOptions
  )
}
