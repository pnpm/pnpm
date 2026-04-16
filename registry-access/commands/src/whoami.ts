import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { rcOptionsTypes } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export interface WhoamiOptions extends CreateFetchFromRegistryOptions {
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export const whoami = {
  cliOptionsTypes,
  commandNames: ['whoami'],
  handler: async (opts: WhoamiOptions): Promise<string> => {
    const registryUrl = normalizeRegistryUrl(opts.registries?.default ?? 'https://registry.npmjs.org/')
    const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)
    const authHeader = getAuthHeader(registryUrl)
    if (!authHeader) {
      throw new PnpmError('WHOAMI_UNAUTHORIZED', 'You must be logged in to use whoami')
    }
    return fetchWhoami(registryUrl, createFetchFromRegistry(opts), authHeader)
  },
  help: (): string => renderHelp({
    description: 'Displays your pnpm username.',
    url: docsUrl('whoami'),
    usages: ['pnpm whoami'],
  }),
  rcOptionsTypes,
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

function normalizeRegistryUrl (registryUrl: string): string {
  return registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
}
