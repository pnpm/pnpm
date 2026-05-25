import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'

export interface SetDistTagOptions {
  packageName: string
  version: string
  distTag: string
  registryUrl: string
  fetchFromRegistry: FetchFromRegistry
  authHeader?: string
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
      ...(opts.otp ? { 'npm-otp': opts.otp } : {}),
    },
    body: JSON.stringify(opts.version),
  })
  if (response.ok) return
  const body = await response.text()
  const action = `set dist-tag "${opts.distTag}" on`
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${action} packages. ${body}`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action} this package. ${body}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${body}`)
}
