import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { fetch } from '@pnpm/network.fetch'
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

export interface DeprecateOptions {
  cliOptions: {
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export function parsePackageSpec (spec: string): { name: string, version: string | undefined } {
  const atIndex = spec.lastIndexOf('@')
  const scopeEndIndex = spec.indexOf('/')

  let name: string
  let version: string | undefined

  if (atIndex > 0 && (scopeEndIndex === -1 || atIndex > scopeEndIndex)) {
    name = spec.substring(0, atIndex)
    version = spec.substring(atIndex + 1)
  } else if (spec.startsWith('@')) {
    const slashIndex = spec.indexOf('/')
    if (slashIndex === -1) {
      throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
    }
    name = spec
    version = undefined
  } else {
    name = spec
    version = undefined
  }

  return { name, version }
}

export async function updateDeprecation (
  packageName: string,
  versionRange: string | undefined,
  message: string,
  opts: DeprecateOptions,
  isUndeprecate: boolean
): Promise<void> {
  const registryUrl = opts.registries?.default ?? 'https://registry.npmjs.org/'

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)

  const authHeader = getAuthHeader(registryUrl)

  const packageUrl = `${registryUrl.replace(/\/$/, '')}/${packageName}`

  const getResponse = await fetch(packageUrl, {
    headers: {
      ...(authHeader ? { authorization: authHeader } : {}),
    },
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

  if (isUndeprecate) {
    const deprecatedVersions = versionsToUpdate.filter((ver) => pkg.versions[ver].deprecated)
    if (deprecatedVersions.length === 0) {
      throw new PnpmError('NOT_DEPRECATED', `No deprecated versions found in "${packageName}"${versionRange ? ` matching "${versionRange}"` : ''}`)
    }
  }

  for (const ver of versionsToUpdate) {
    const verData = pkg.versions[ver]
    if (message === '') {
      delete verData.deprecated
    } else {
      verData.deprecated = message
    }
  }

  const otp = opts.cliOptions?.otp

  const putResponse = await fetch(packageUrl, {
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
      throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${isUndeprecate ? 'un-deprecate' : 'deprecate'} packages`)
    }
    if (putResponse.status === 403) {
      throw new PnpmError('FORBIDDEN', `You do not have permission to ${isUndeprecate ? 'un-deprecate' : 'deprecate'} this package`)
    }
    const errorBody = await putResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to ${isUndeprecate ? 'un-deprecate' : 'deprecate'} package: ${putResponse.status} ${putResponse.statusText}. ${errorBody}`)
  }

  globalInfo(`Successfully ${isUndeprecate ? 'un-deprecated' : 'deprecated'} ${versionsToUpdate.length} version(s) of ${packageName}`)
}

function getVersionsMatchingRange (
  versions: Record<string, PackageInRegistry>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
