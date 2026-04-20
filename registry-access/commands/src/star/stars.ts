import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { renderHelp } from 'render-help'

import { normalizeRegistryUrl } from '../common.js'
import { fetchWhoami } from '../whoami.js'
import { cliOptionsTypes, getAuthHeaderForRegistry, rcOptionsTypes, type StarOptions } from './common.js'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['stars']

export function help (): string {
  return renderHelp({
    description: 'Lists all packages starred by a specific user.',
    url: docsUrl('stars'),
    usages: ['pnpm stars [<user>]'],
  })
}

export async function handler (opts: StarOptions, params: string[]): Promise<string> {
  const registryUrl = normalizeRegistryUrl(opts.registries?.default ?? 'https://registry.npmjs.org/')
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)

  let username = params[0]
  if (!username) {
    if (!authHeader) {
      throw new PnpmError('STARS_UNAUTHORIZED', 'You must be logged in to list your starred packages')
    }
    username = await fetchWhoami(registryUrl, fetchFromRegistry, authHeader)
  }

  if (!params[0]) {
    const starUrl = new URL('./-/user/v1/star', registryUrl).href
    const response = await fetchFromRegistry(starUrl, {
      authHeaderValue: authHeader,
    })
    if (response.ok) {
      const starsData = await response.json() as string[] | Record<string, unknown>
      if (Array.isArray(starsData)) return starsData.join('\n')
      if (typeof starsData === 'object' && starsData !== null) {
        return Object.keys(starsData).join('\n')
      }
    }
  }

  const starsUrl = new URL(`./-/user/${encodeURIComponent(username)}/stars`, registryUrl).href
  let response = await fetchFromRegistry(starsUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    const utilStarsUrl = new URL(`./-/util/user/${encodeURIComponent(username)}/stars`, registryUrl).href
    response = await fetchFromRegistry(utilStarsUrl, {
      authHeaderValue: authHeader,
    })
  }

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('USER_NOT_FOUND', `User "${username}" not found`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch stars: ${response.status} ${response.statusText}`)
  }

  const starsData = await response.json() as Record<string, unknown>
  return Object.keys(starsData).join('\n')
}
