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
            description: 'The base URL of the npm registry. If specified, this registry will be used for all package operations.',
            name: '--registry <url>',
          },
          {
            description: 'When publishing packages that require two-factor authentication, this option can specify a one-time password.',
            name: '--otp',
          },
          {
            description: 'Removes the package from the registry regardless of what version is currently published. Without this flag, pnpm will prompt to confirm the unpublish if the package has dependents in the registry.',
            name: '--force',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('unpublish'),
    usages: [
      'pnpm unpublish [<package>[@<version>]',
    ],
  })
}

interface UnpublishOptions {
  argv: {
    original: string[]
  }
  cliOptions: {
    force?: boolean
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
  opts: UnpublishOptions,
  params: string[]
): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('UNPUBLISH_REQUIRED', 'Package name is required')
  }

  const packageSpec = params[0]
  const { name, version } = parsePackageSpec(packageSpec)

  await unpublishPackage(name, version, opts)
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

async function unpublishPackage (
  packageName: string,
  version: string | undefined,
  opts: UnpublishOptions
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

  if (version) {
    const versionsToUnpublish = getVersionsMatchingRange(pkg.versions, version)
    if (versionsToUnpublish.length === 0) {
      throw new PnpmError('NO_MATCHING_VERSIONS', `No versions match "${version}"`)
    }
    await unpublishVersions(packageUrl, pkg, versionsToUnpublish, opts, authHeader)
  } else {
    await unpublishAll(packageUrl, pkg, opts, authHeader)
  }
}

async function unpublishVersions (
  packageUrl: string,
  pkg: PackageManifest,
  versionsToUnpublish: string[],
  opts: UnpublishOptions,
  authHeader: string | undefined
): Promise<void> {
  for (const ver of versionsToUnpublish) {
    delete pkg.versions![ver]
  }

  delete pkg.time

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
      throw new PnpmError('UNAUTHORIZED', 'You must be logged in to unpublish packages')
    }
    if (putResponse.status === 403) {
      throw new PnpmError('FORBIDDEN', 'You do not have permission to unpublish this package')
    }
    const errorBody = await putResponse.text()
    throw new PnpmError('REGISTRY_ERROR', `Failed to unpublish package: ${putResponse.status} ${putResponse.statusText}. ${errorBody}`)
  }

  console.log(`Successfully unpublished ${versionsToUnpublish.length} version(s) of ${pkg.name}`)
}

async function unpublishAll (
  packageUrl: string,
  pkg: PackageManifest,
  opts: UnpublishOptions,
  authHeader: string | undefined
): Promise<void> {
  const packageName = pkg.name
  const versionCount = Object.keys(pkg.versions ?? {}).length
  const force = opts.cliOptions?.force ?? false

  if (!force) {
    const versionsList = Object.keys(pkg.versions ?? {}).join(', ')
    throw new PnpmError('UNPUBLISH_CONFIRM', `Run pnpm unpublish --force to remove all published versions of ${packageName} (${versionsList}) from the registry.
This is a protection mechanism to prevent accidental unpublish of packages with many versions.
If you want to unpublish a specific version, run pnpm unpublish ${packageName}@<version>`)
  }

  const deleteResponse = await fetch(packageUrl, {
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

  console.log(`Successfully unpublished all ${versionCount} version(s) of ${packageName}`)
}

function getVersionsMatchingRange (
  versions: Record<string, PackageVersion>,
  range: string
): string[] {
  return Object.keys(versions).filter((v) => semver.satisfies(v, range))
}
