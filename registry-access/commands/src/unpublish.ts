import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'
import semver from 'semver'

import { normalizeRegistryUrl, parsePackageSpec, rcOptionsTypes } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    force: Boolean,
    otp: String,
  }
}

export const commandNames = ['unpublish']

export function help (): string {
  return renderHelp({
    description: 'Removes a package from the registry.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'The base URL of the npm registry.',
            name: '--registry <url>',
          },
          {
            description: 'When publishing packages that require two-factor authentication, this option can specify a one-time password.',
            name: '--otp',
          },
          {
            description: 'Removes the package from the registry regardless of what version is currently published. Without this flag, pnpm will refuse to unpublish an entire package.',
            name: '--force',
          },
        ],
      },
    ],
    url: docsUrl('unpublish'),
    usages: [
      'pnpm unpublish [<package>[@<version>]]',
    ],
  })
}

export interface UnpublishOptions extends CreateFetchFromRegistryOptions {
  cliOptions?: {
    force?: boolean
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

interface PackumentResponse {
  _id?: string
  _rev?: string
  name: string
  'dist-tags': Record<string, string>
  versions: Record<string, VersionData>
  time?: Record<string, string>
  _revisions?: unknown
  _attachments?: unknown
  [key: string]: unknown
}

interface VersionData {
  name: string
  version: string
  dist: {
    tarball: string
    [key: string]: unknown
  }
  [key: string]: unknown
}

export async function handler (
  opts: UnpublishOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('UNPUBLISH_REQUIRED', 'Package name is required')
  }

  const packageSpec = params[0]
  const { name, versionRange } = parsePackageSpec(packageSpec)

  return unpublishPackage(name, versionRange, opts)
}

async function unpublishPackage (
  packageName: string,
  versionRange: string | undefined,
  opts: UnpublishOptions
): Promise<string> {
  const registryUrl = normalizeRegistryUrl(pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName))

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)

  const authHeader = getAuthHeader(registryUrl)

  const packageUrl = new URL(npa(packageName).escapedName, registryUrl).href

  const fetchFromRegistry = createFetchFromRegistry(opts)
  const pkg = await fetchPackument(packageUrl, fetchFromRegistry, authHeader)

  const allVersions = pkg.versions
  if (!allVersions || Object.keys(allVersions).length === 0) {
    throw new PnpmError('NO_VERSIONS', `Package "${packageName}" has no versions`)
  }

  const otp = opts.cliOptions?.otp

  if (!versionRange) {
    return unpublishAll(packageUrl, pkg, fetchFromRegistry, authHeader, otp, opts.cliOptions)
  }

  const versionsToUnpublish = getVersionsMatchingRange(allVersions, versionRange)
  if (versionsToUnpublish.length === 0) {
    throw new PnpmError('NO_MATCHING_VERSIONS', `No versions match "${versionRange}"`)
  }

  // If removing all matched versions leaves none, treat as full unpublish
  if (versionsToUnpublish.length === Object.keys(allVersions).length) {
    return unpublishAll(packageUrl, pkg, fetchFromRegistry, authHeader, otp, opts.cliOptions)
  }

  return unpublishVersions(packageUrl, registryUrl, pkg, versionsToUnpublish, fetchFromRegistry, authHeader, otp)
}

async function fetchPackument (
  packageUrl: string,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined
): Promise<PackumentResponse> {
  const response = await fetchFromRegistry(packageUrl, {
    authHeaderValue: authHeader,
    fullMetadata: true,
  })

  if (!response.ok) {
    if (response.status === 404) {
      const url = new URL(packageUrl)
      const packageName = decodeURIComponent(url.pathname.split('/').pop()!)
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch package info: ${response.status} ${response.statusText}`)
  }

  return await response.json() as PackumentResponse
}

async function unpublishVersions (
  packageUrl: string,
  registryUrl: string,
  pkg: PackumentResponse,
  versions: string[],
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined,
  otp: string | undefined
): Promise<string> {
  // Collect tarball URLs before mutating
  const tarballs: string[] = []
  for (const version of versions) {
    const versionData = pkg.versions[version]
    if (versionData?.dist?.tarball) {
      tarballs.push(versionData.dist.tarball)
    }
    delete pkg.versions[version]
  }

  // Update dist-tags: remove any tag pointing to removed versions
  const removedSet = new Set(versions)
  const latestVer = pkg['dist-tags'].latest
  for (const tag of Object.keys(pkg['dist-tags'])) {
    if (removedSet.has(pkg['dist-tags'][tag])) {
      delete pkg['dist-tags'][tag]
    }
  }

  // If we removed 'latest', reassign it to the highest remaining version
  if (latestVer && removedSet.has(latestVer)) {
    const remaining = Object.keys(pkg.versions).sort(semver.compareLoose)
    if (remaining.length > 0) {
      pkg['dist-tags'].latest = remaining[remaining.length - 1]
    }
  }

  // Clean up internal metadata
  delete pkg._revisions
  delete pkg._attachments

  // PUT updated packument
  const putResponse = await fetchFromRegistry(`${packageUrl}/-rev/${pkg._rev}`, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify(pkg),
  })

  if (!putResponse.ok) {
    await throwRegistryError(putResponse, 'unpublish')
  }

  // Delete each tarball
  const registryOrigin = new URL(registryUrl).origin
  /* eslint-disable no-await-in-loop */
  for (const tarball of tarballs) {
    const updated = await fetchPackument(packageUrl, fetchFromRegistry, authHeader)
    const tarballPathname = getTarballPathname(tarball, registryUrl)
    const deleteResponse = await fetchFromRegistry(`${registryOrigin}/${tarballPathname}/-rev/${updated._rev}`, {
      authHeaderValue: authHeader,
      method: 'DELETE',
      headers: {
        ...(otp ? { 'npm-otp': otp } : {}),
      },
    })

    // Some registries handle tarball cleanup automatically on packument update,
    // so treat 404 as success.
    if (!deleteResponse.ok && deleteResponse.status !== 404) {
      await throwRegistryError(deleteResponse, 'unpublish')
    }
  }
  /* eslint-enable no-await-in-loop */

  return `Successfully unpublished ${versions.length} version(s) of ${pkg.name}`
}

async function unpublishAll (
  packageUrl: string,
  pkg: PackumentResponse,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined,
  otp: string | undefined,
  cliOptions: UnpublishOptions['cliOptions']
): Promise<string> {
  const packageName = pkg.name
  const versionCount = Object.keys(pkg.versions).length
  const force = cliOptions?.force ?? false

  if (!force) {
    const versionsList = Object.keys(pkg.versions).join(', ')
    throw new PnpmError('UNPUBLISH_CONFIRM', `Run pnpm unpublish --force to remove all published versions of ${packageName} (${versionsList}) from the registry.
This is a protection mechanism to prevent accidental unpublish of packages with many versions.
If you want to unpublish a specific version, run pnpm unpublish ${packageName}@<version>`)
  }

  const deleteResponse = await fetchFromRegistry(`${packageUrl}/-rev/${pkg._rev}`, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      ...(otp ? { 'npm-otp': otp } : {}),
    },
  })

  if (!deleteResponse.ok) {
    if (deleteResponse.status === 405) {
      throw new PnpmError('UNPUBLISH_FORBIDDEN', 'This package cannot be completely unpublished. Deprecate it instead or contact npm support.')
    }
    await throwRegistryError(deleteResponse, 'unpublish')
  }

  return `Successfully unpublished all ${versionCount} version(s) of ${packageName}`
}

async function throwRegistryError (response: Response, verb: string): Promise<never> {
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${verb} packages`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${verb} this package`)
  }
  const errorBody = await response.text()
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${verb} package: ${response.status} ${response.statusText}. ${errorBody}`)
}

function getTarballPathname (tarballUrl: string, registryUrl: string): string {
  const registryPath = new URL(registryUrl).pathname.slice(1)
  let tarballPath = new URL(tarballUrl).pathname.slice(1)
  if (registryPath && tarballPath.startsWith(registryPath)) {
    tarballPath = tarballPath.slice(registryPath.length)
  }
  return tarballPath
}

function getVersionsMatchingRange (
  versions: Record<string, VersionData>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
