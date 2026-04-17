import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'

import { normalizeRegistryUrl, parsePackageSpec, rcOptionsTypes as commonRcOptionsTypes } from '../common.js'
import { fetchWhoami } from '../whoami.js'

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...commonRcOptionsTypes(),
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return commonRcOptionsTypes()
}

export interface StarOptions extends CreateFetchFromRegistryOptions {
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

interface StarActionArgs {
  packageName: string
  star: boolean
}

interface PackumentWithStars {
  _rev?: string
  users?: Record<string, boolean>
  [key: string]: unknown
}

export async function performStarAction (opts: StarOptions, { packageName, star }: StarActionArgs): Promise<void> {
  const { escapedName } = parsePackageSpec(packageName)
  const registryUrl = normalizeRegistryUrl(
    pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)
  )
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)
  const action = star ? 'star' : 'unstar'
  if (!authHeader) {
    throw new PnpmError('STAR_UNAUTHORIZED', `You must be logged in to ${action} packages`)
  }
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const method = star ? 'PUT' : 'DELETE'

  const starUrl = new URL('./-/user/v1/star', registryUrl).href

  let response = await fetchFromRegistry(starUrl, {
    authHeaderValue: authHeader,
    method,
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({ name: packageName, package: packageName }),
  })

  if (!response.ok) {
    const altStarUrl = new URL(`./-/user/package/${escapedName}/star`, registryUrl).href
    response = await fetchFromRegistry(altStarUrl, {
      authHeaderValue: authHeader,
      method,
      headers: {
        'content-type': 'application/json',
      },
    })
  }

  if (!response.ok) {
    if (response.status === 404 || response.status === 405 || response.status === 400 || response.status === 500) {
      return performLegacyStarAction({ packageName, escapedName, star, registryUrl, authHeader, fetchFromRegistry })
    }
    const errorBody = await response.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${errorBody}`)
  }
}

interface LegacyStarActionArgs {
  packageName: string
  escapedName: string
  star: boolean
  registryUrl: string
  authHeader: string
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
}

async function performLegacyStarAction (args: LegacyStarActionArgs): Promise<void> {
  const { packageName, escapedName, star, registryUrl, authHeader, fetchFromRegistry } = args
  const action = star ? 'star' : 'unstar'

  const username = await fetchWhoami(registryUrl, fetchFromRegistry, authHeader)
  const pkgUrl = new URL(`./${escapedName}`, registryUrl).href

  const response = await fetchFromRegistry(pkgUrl, {
    authHeaderValue: authHeader,
    fullMetadata: true,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch package info: ${response.status} ${response.statusText}`)
  }

  const pkgData = await response.json() as PackumentWithStars
  pkgData.users = pkgData.users || {}
  if (star) {
    pkgData.users[username] = true
  } else {
    delete pkgData.users[username]
  }

  const updateUrl = pkgData._rev ? `${pkgUrl}/-rev/${pkgData._rev}` : pkgUrl
  const updateResponse = await fetchFromRegistry(updateUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify(pkgData),
  })

  if (!updateResponse.ok) {
    const errorBody = await updateResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package (legacy): ${updateResponse.status} ${updateResponse.statusText}. ${errorBody}`)
  }
}

export function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {}, registryUrl)
  return getAuthHeader(registryUrl)
}
