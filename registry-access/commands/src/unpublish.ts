import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, fetchWithDispatcher } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import semver from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'registry',
  ], allTypes)
}

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
  cliOptions: {
    force?: boolean
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

interface PackageManifest {
  _id?: string
  _rev?: string
  name: string
  description?: string
  'dist-tags'?: Record<string, string>
  versions?: Record<string, PackageVersion>
  time?: Record<string, string>
  [key: string]: unknown
}

interface PackageVersion {
  name: string
  version: string
  deprecated?: string | true
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

function parsePackageSpec (spec: string): { name: string, versionRange: string | undefined } {
  let parsed: ReturnType<typeof npa>
  try {
    parsed = npa(spec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  if (!parsed.name) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  const versionRange = parsed.rawSpec || undefined
  return { name: parsed.name, versionRange }
}

async function unpublishPackage (
  packageName: string,
  versionRange: string | undefined,
  opts: UnpublishOptions
): Promise<string> {
  const registryUrl = pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)

  const authHeader = getAuthHeader(registryUrl)

  const packageUrl = new URL(encodeURIComponent(packageName), registryUrl).href

  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getResponse = await fetchFromRegistry(packageUrl, {
    authHeaderValue: authHeader,
    fullMetadata: true,
  })

  if (!getResponse.ok) {
    if (getResponse.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch package info: ${getResponse.status} ${getResponse.statusText}`)
  }

  const pkg: PackageManifest = await getResponse.json() as PackageManifest

  if (!pkg.versions || Object.keys(pkg.versions).length === 0) {
    throw new PnpmError('NO_VERSIONS', `Package "${packageName}" has no versions`)
  }

  if (versionRange) {
    const versionsToUnpublish = getVersionsMatchingRange(pkg.versions, versionRange)
    if (versionsToUnpublish.length === 0) {
      throw new PnpmError('NO_MATCHING_VERSIONS', `No versions match "${versionRange}"`)
    }
    return unpublishVersions(packageUrl, pkg, versionsToUnpublish, opts, authHeader)
  } else {
    return unpublishAll(packageUrl, pkg, opts, authHeader)
  }
}

async function unpublishVersions (
  packageUrl: string,
  pkg: PackageManifest,
  versionsToUnpublish: string[],
  opts: UnpublishOptions,
  authHeader: string | undefined
): Promise<string> {
  for (const ver of versionsToUnpublish) {
    delete pkg.versions![ver]
  }

  delete pkg.time

  const otp = opts.cliOptions?.otp

  const putResponse = await fetchWithDispatcher(packageUrl, {
    dispatcherOptions: opts,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(authHeader ? { authorization: authHeader } : {}),
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify(pkg),
  })

  if (!putResponse.ok) {
    if (putResponse.status === 401) {
      throw new PnpmError('UNAUTHORIZED', 'You must be logged in to unpublish packages')
    }
    if (putResponse.status === 403) {
      throw new PnpmError('FORBIDDEN', 'You do not have permission to unpublish this package')
    }
    const errorBody = await putResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to unpublish package: ${putResponse.status} ${putResponse.statusText}. ${errorBody}`)
  }

  return `Successfully unpublished ${versionsToUnpublish.length} version(s) of ${pkg.name}`
}

async function unpublishAll (
  packageUrl: string,
  pkg: PackageManifest,
  opts: UnpublishOptions,
  authHeader: string | undefined
): Promise<string> {
  const packageName = pkg.name
  const versionCount = Object.keys(pkg.versions ?? {}).length
  const force = opts.cliOptions?.force ?? false

  if (!force) {
    const versionsList = Object.keys(pkg.versions ?? {}).join(', ')
    throw new PnpmError('UNPUBLISH_CONFIRM', `Run pnpm unpublish --force to remove all published versions of ${packageName} (${versionsList}) from the registry.
This is a protection mechanism to prevent accidental unpublish of packages with many versions.
If you want to unpublish a specific version, run pnpm unpublish ${packageName}@<version>`)
  }

  const deleteResponse = await fetchWithDispatcher(packageUrl, {
    dispatcherOptions: opts,
    method: 'DELETE',
    headers: {
      ...(authHeader ? { authorization: authHeader } : {}),
      ...(opts.cliOptions?.otp ? { 'npm-otp': opts.cliOptions.otp } : {}),
    },
  })

  if (!deleteResponse.ok) {
    if (deleteResponse.status === 401) {
      throw new PnpmError('UNAUTHORIZED', 'You must be logged in to unpublish packages')
    }
    if (deleteResponse.status === 403) {
      throw new PnpmError('FORBIDDEN', 'You do not have permission to unpublish this package')
    }
    if (deleteResponse.status === 405) {
      throw new PnpmError('UNPUBLISH_FORBIDDEN', 'This package cannot be completely unpublished. Deprecate it instead or contact npm support.')
    }
    const errorBody = await deleteResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to unpublish package: ${deleteResponse.status} ${deleteResponse.statusText}. ${errorBody}`)
  }

  return `Successfully unpublished all ${versionCount} version(s) of ${packageName}`
}

function getVersionsMatchingRange (
  versions: Record<string, PackageVersion>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
