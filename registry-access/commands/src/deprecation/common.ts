import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, fetchWithDispatcher } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { pick } from 'ramda'
import semver from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'registry',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    otp: String,
  }
}

export interface DeprecateOptions extends CreateFetchFromRegistryOptions {
  cliOptions: {
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export function parsePackageSpec (spec: string): { name: string, versionRange: string | undefined } {
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

interface UpdateDeprecationOptions {
  deprecated: string | undefined
  packageName: string
  versionRange: string | undefined
}

export async function updateDeprecation (
  opts: DeprecateOptions,
  { deprecated, packageName, versionRange }: UpdateDeprecationOptions
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

  const pkg = await getResponse.json() as PackageMeta

  if (!pkg.versions || Object.keys(pkg.versions).length === 0) {
    throw new PnpmError('NO_VERSIONS', `Package "${packageName}" has no versions`)
  }

  const versionsToUpdate = versionRange
    ? getVersionsMatchingRange(pkg.versions, versionRange)
    : Object.keys(pkg.versions)

  if (versionsToUpdate.length === 0) {
    throw new PnpmError('NO_MATCHING_VERSIONS', `No versions match "${versionRange}"`)
  }

  if (deprecated == null) {
    const deprecatedVersions = versionsToUpdate.filter((ver) => pkg.versions[ver].deprecated)
    if (deprecatedVersions.length === 0) {
      throw new PnpmError('NOT_DEPRECATED', `No deprecated versions found in "${packageName}"${versionRange ? ` matching "${versionRange}"` : ''}`)
    }
  }

  const updatedVersions: Record<string, { deprecated: string }> = {}
  for (const ver of versionsToUpdate) {
    updatedVersions[ver] = { deprecated: deprecated ?? '' }
  }

  const otp = opts.cliOptions?.otp

  const putResponse = await fetchWithDispatcher(packageUrl, {
    dispatcherOptions: opts,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(authHeader ? { authorization: authHeader } : {}),
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({
      name: packageName,
      versions: updatedVersions,
    }),
  })

  if (!putResponse.ok) {
    const verb = deprecated != null ? 'deprecate' : 'undeprecate'
    if (putResponse.status === 401) {
      throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${verb} packages`)
    }
    if (putResponse.status === 403) {
      throw new PnpmError('FORBIDDEN', `You do not have permission to ${verb} this package`)
    }
    const errorBody = await putResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${verb} package: ${putResponse.status} ${putResponse.statusText}. ${errorBody}`)
  }

  return `Successfully ${deprecated != null ? 'deprecated' : 'un-deprecated'} ${versionsToUpdate.length} version(s) of ${packageName}`
}

function getVersionsMatchingRange (
  versions: Record<string, PackageInRegistry>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
