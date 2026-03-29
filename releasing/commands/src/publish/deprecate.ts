import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { fetch } from '@pnpm/network.fetch'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import semver from 'semver'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'registry',
    'npm-path',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    otp: String,
  }
}

export const commandNames = ['deprecate', 'undeprecate']

export function help (): string {
  return renderHelp({
    description: 'Deprecates or un-deprecates a version of a package in the registry.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'Deprecates a package version with a message.',
            name: 'deprecate <package>[@<version>] <message>',
          },
          {
            description: 'Removes deprecation from a package version. Only works on already deprecated versions.',
            name: 'undeprecate <package>[@<version>]',
          },
        ],
      },
      {
        title: 'Options',

        list: [
          {
            description: 'The base URL of the npm registry. If specified, this registry will be used for all package operations.',
            name: '--registry <url>',
          },
          {
            description: 'When publishing packages that require two-factor authentication, this option can specify a one-time password.',
            name: '--otp',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('deprecate'),
    usages: [
      'pnpm deprecate <package>[@<version>] <message>',
      'pnpm undeprecate <package>[@<version>]',
    ],
  })
}

interface DeprecateOptions {
  argv: {
    original: string[]
  }
  cliOptions: {
    otp?: string
  }
  registry?: string
  rawConfig: Record<string, unknown>
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
  opts: DeprecateOptions,
  params: string[]
): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('DEPRECATE_REQUIRED', 'Package name is required')
  }

  const packageSpec = params[0]
  const isUndeprecate = opts.argv.original[0] === 'undeprecate'
  const message = params.length > 1 ? params.slice(1).join(' ') : ''

  if (isUndeprecate && message !== '') {
    throw new PnpmError('UNDEPRECATE_NO_MESSAGE', 'The undeprecate command does not accept a message. Use deprecate with an empty message instead.')
  }

  const { name, version } = parsePackageSpec(packageSpec)

  await deprecatePackage(name, version, message, opts, isUndeprecate)
}

function parsePackageSpec (spec: string): { name: string, version: string | undefined } {
  const atIndex = spec.lastIndexOf('@')
  const scopeEndIndex = spec.indexOf('/')

  let name: string
  let version: string | undefined

  if (atIndex > 0 && (scopeEndIndex === -1 || atIndex < scopeEndIndex)) {
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

async function deprecatePackage (
  packageName: string,
  versionRange: string | undefined,
  message: string,
  opts: DeprecateOptions,
  isUndeprecate: boolean
): Promise<void> {
  const registryUrl = opts.registry ?? 'https://registry.npmjs.org'

  const getAuthHeader = createGetAuthHeaderByURI({
    allSettings: opts.rawConfig as Record<string, string>,
    userSettings: {},
  })

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

  const pkg: PackageManifest = await getResponse.json() as PackageManifest

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
    const deprecatedVersions = versionsToUpdate.filter((ver) => pkg.versions![ver].deprecated)
    if (deprecatedVersions.length === 0) {
      throw new PnpmError('NOT_DEPRECATED', `No deprecated versions found in "${packageName}"${versionRange ? ` matching "${versionRange}"` : ''}`)
    }
  }

  for (const ver of versionsToUpdate) {
    const verData = pkg.versions![ver]
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

  console.log(`Successfully ${isUndeprecate ? 'un-deprecated' : 'deprecated'} ${versionsToUpdate.length} version(s) of ${packageName}`)
}

function getVersionsMatchingRange (
  versions: Record<string, PackageVersion>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
