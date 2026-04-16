import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { rcOptionsTypes } from './common.js'
import { fetchWhoami } from './whoami.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export interface StarOptions extends CreateFetchFromRegistryOptions {
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export const star = {
  cliOptionsTypes,
  commandNames: ['star'],
  handler: starHandler,
  help: (): string => renderHelp({
    description: 'Marks a package as a favorite.',
    url: docsUrl('star'),
    usages: ['pnpm star <package>'],
  }),
  rcOptionsTypes,
}

export const unstar = {
  cliOptionsTypes,
  commandNames: ['unstar'],
  handler: unstarHandler,
  help: (): string => renderHelp({
    description: 'Removes a package from your favorites.',
    url: docsUrl('unstar'),
    usages: ['pnpm unstar <package>'],
  }),
  rcOptionsTypes,
}

export const stars = {
  cliOptionsTypes,
  commandNames: ['stars'],
  handler: starsHandler,
  help: (): string => renderHelp({
    description: 'Lists all packages starred by a specific user.',
    url: docsUrl('stars'),
    usages: ['pnpm stars [<user>]'],
  }),
  rcOptionsTypes,
}

async function starHandler (opts: StarOptions, params: string[]): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('STAR_PACKAGE_REQUIRED', 'Package name is required')
  }
  await performStarAction(opts, params[0], true)
}

async function unstarHandler (opts: StarOptions, params: string[]): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('UNSTAR_PACKAGE_REQUIRED', 'Package name is required')
  }
  await performStarAction(opts, params[0], false)
}

async function performStarAction (opts: StarOptions, packageName: string, star: boolean): Promise<void> {
  const encodedName = parsePackageSpecName(packageName)
  const registryUrl = normalizeRegistryUrl(
    pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)
  )
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)
  if (!authHeader) {
    throw new PnpmError('STAR_UNAUTHORIZED', `You must be logged in to ${star ? 'star' : 'unstar'} packages`)
  }
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const starUrl = new URL('./-/user/v1/star', registryUrl).href

  let response = await fetchFromRegistry(starUrl, {
    authHeaderValue: authHeader,
    method: star ? 'PUT' : 'DELETE',
    headers: {
      'content-type': 'application/json',
    },
    body: JSON.stringify({ name: packageName, package: packageName }),
  })

  if (!response.ok) {
    const altStarUrl = new URL(`./-/user/package/${encodedName}/star`, registryUrl).href
    response = await fetchFromRegistry(altStarUrl, {
      authHeaderValue: authHeader,
      method: star ? 'PUT' : 'DELETE',
      headers: {
        'content-type': 'application/json',
      },
    })
  }

  if (!response.ok) {
    if (response.status === 404 || response.status === 405 || response.status === 400 || response.status === 500) {
      return performLegacyStarAction({ opts, packageName, encodedName, star, registryUrl, authHeader, fetchFromRegistry })
    }
    const errorBody = await response.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${star ? 'star' : 'unstar'} package: ${response.status} ${response.statusText}. ${errorBody}`)
  }
}

interface PackumentWithStars {
  _rev?: string
  users?: Record<string, boolean>
  [key: string]: unknown
}

interface LegacyStarActionArgs {
  opts: StarOptions
  packageName: string
  encodedName: string
  star: boolean
  registryUrl: string
  authHeader: string
  fetchFromRegistry: ReturnType<typeof createFetchFromRegistry>
}

async function performLegacyStarAction (args: LegacyStarActionArgs): Promise<void> {
  const { packageName, encodedName, star, registryUrl, authHeader, fetchFromRegistry } = args

  const username = await fetchWhoami(registryUrl, fetchFromRegistry, authHeader)
  const pkgUrl = new URL(`./${encodedName}`, registryUrl).href

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
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${star ? 'star' : 'unstar'} package (legacy): ${updateResponse.status} ${updateResponse.statusText}. ${errorBody}`)
  }
}

async function starsHandler (opts: StarOptions, params: string[]): Promise<string> {
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

function parsePackageSpecName (packageName: string): string {
  let parsed
  try {
    parsed = npa(packageName)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${packageName}`)
  }
  if (!parsed.escapedName) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${packageName}`)
  }
  return parsed.escapedName
}

function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {}, registryUrl)
  return getAuthHeader(registryUrl)
}

function normalizeRegistryUrl (registryUrl: string): string {
  return registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
}
