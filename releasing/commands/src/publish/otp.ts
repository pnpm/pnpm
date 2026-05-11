import {
  type OtpContext as BaseOtpContext,
  type WebAuthFetchOptions,
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

export interface OtpContext extends BaseOtpContext {
  publish: OtpPublishFn
}

export interface OtpParams {
  context?: OtpContext
  manifest: ExportedManifest
  publishOptions: PublishOptions
  tarballData: Buffer
}

/**
 * Publish a package, handling OTP challenges:
 * - Web based authentication flow (authUrl/doneUrl in error body with doneUrl polling)
 * - Classic OTP prompt (manual code entry)
 *
 * The caller is responsible for supplying a {@link OtpContext.fetch} that
 * honors the desired network configuration (proxy, TLS, etc.); see
 * https://github.com/pnpm/pnpm/issues/11561 for why this matters during the
 * web-based authentication flow.
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
  const { publish } = context

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

  return withOtpHandling({
    context,
    fetchOptions,
    // When otp is undefined (first attempt), { ...publishOptions, otp } adds
    // otp: undefined to the options. This is safe because libnpmpublish treats
    // undefined the same as absent (unlike HTTP headers, where undefined gets
    // coerced to the string "undefined").
    operation: otp => publish(manifest, tarballData, { ...publishOptions, otp }),
  })
}
