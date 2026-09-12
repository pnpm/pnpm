import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/network.fetch'
import { SyntheticOtpError } from '@pnpm/network.web-auth'
import npa from '@pnpm/npm-package-arg'

export type AuthType = 'web' | 'legacy'

export interface SetDistTagOptions {
  packageName: string
  version: string
  distTag: string
  registryUrl: string
  fetchFromRegistry: FetchFromRegistry
  authHeader?: string
  /** Mirrors npm CLI's `auth-type` flat option: `'web'` (default) opts into the
   * web-OTP challenge, `'legacy'` is set automatically when the user passes
   * `--otp` so a classic 6-digit code can be sent. */
  authType?: AuthType
  /** OTP token to send as `npm-otp`. May be a classic 6-digit code (legacy) or
   * the 64-character token returned by the web flow. */
  otp?: string
}

export async function setDistTag (opts: SetDistTagOptions): Promise<void> {
  const encodedName = npa(opts.packageName).escapedName
  const url = new URL(`-/package/${encodedName}/dist-tags/${encodeURIComponent(opts.distTag)}`, opts.registryUrl).href
  const response = await opts.fetchFromRegistry(url, {
    authHeaderValue: opts.authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      'npm-auth-type': opts.authType ?? 'web',
      ...(opts.otp ? { 'npm-otp': opts.otp } : {}),
    },
    body: JSON.stringify(opts.version),
  })
  if (response.ok) return
  const body = await response.text()
  const action = `set dist-tag "${opts.distTag}" on`
  if (response.status === 401) {
    throw SyntheticOtpError.fromUnauthorizedBody(body) ??
      new PnpmError('UNAUTHORIZED', `You must be logged in to ${action} packages. ${body}`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action} this package. ${body}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${body}`)
}

