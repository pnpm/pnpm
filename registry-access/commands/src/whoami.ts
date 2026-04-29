import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { normalizeRegistryUrl, rcOptionsTypes as commonRcOptionsTypes } from './common.js'

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...commonRcOptionsTypes(),
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return commonRcOptionsTypes()
}

export interface WhoamiOptions extends CreateFetchFromRegistryOptions {
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export const commandNames = ['whoami']

export function help (): string {
  return renderHelp({
    description: 'Displays your pnpm username.',
    url: docsUrl('whoami'),
    usages: ['pnpm whoami'],
  })
}

export async function handler (opts: WhoamiOptions): Promise<string> {
  const registryUrl = normalizeRegistryUrl(opts.registries?.default ?? 'https://registry.npmjs.org/')
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)
  const authHeader = getAuthHeader(registryUrl)
  if (!authHeader) {
    throw new PnpmError('WHOAMI_UNAUTHORIZED', 'You must be logged in to use whoami')
  }
  return fetchWhoami(registryUrl, createFetchFromRegistry(opts), authHeader)
}

export async function fetchWhoami (registryUrl: string, fetchFromRegistry: FetchFromRegistry, authHeader: string): Promise<string> {
  const whoamiUrl = new URL('./-/whoami', normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(whoamiUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    throw new PnpmError('WHOAMI_FAILED', `Failed to find the current user: ${response.status} ${response.statusText}`)
  }

  const { username } = await response.json() as { username: string }
  return username
}
