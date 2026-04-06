import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'
import semver from 'semver'

import { parsePackageSpec, rcOptionsTypes } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    otp: String,
  }
}

export const commandNames = ['dist-tag']

export function help (): string {
  return renderHelp({
    description: 'Manages distribution tags for a package.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'List all dist-tags for a package. Default if no subcommand is given.',
            name: 'ls',
          },
          {
            description: 'Add a dist-tag to a specific version of a package.',
            name: 'add',
          },
          {
            description: 'Remove a dist-tag from a package.',
            name: 'rm',
          },
        ],
      },
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
        ],
      },
    ],
    url: docsUrl('dist-tag'),
    usages: [
      'pnpm dist-tag ls [<package>]',
      'pnpm dist-tag add <package>@<version> [<tag>]',
      'pnpm dist-tag rm <package> <tag>',
    ],
  })
}

export interface DistTagOptions extends CreateFetchFromRegistryOptions {
  cliOptions: {
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export async function handler (
  opts: DistTagOptions,
  params: string[]
): Promise<string> {
  const subcommand = params[0]

  if (subcommand === 'add') {
    return distTagAdd(opts, params.slice(1))
  }
  if (subcommand === 'rm') {
    return distTagRm(opts, params.slice(1))
  }
  if (subcommand === 'ls' || subcommand === 'list') {
    return distTagLs(opts, params.slice(1))
  }
  // Default: treat all params as arguments to ls
  return distTagLs(opts, params)
}

async function distTagLs (
  opts: DistTagOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('DIST_TAG_LS_PACKAGE_REQUIRED', 'Package name is required')
  }

  const packageName = params[0]
  const registryUrl = pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)
  const authHeader = getAuthHeader(registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const distTags = await fetchDistTags(packageName, registryUrl, fetchFromRegistry, authHeader)

  const lines: string[] = []
  for (const [tag, version] of Object.entries(distTags).sort(([a], [b]) => a.localeCompare(b))) {
    lines.push(`${tag}: ${version}`)
  }
  return lines.join('\n')
}

async function distTagAdd (
  opts: DistTagOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('DIST_TAG_ADD_SPEC_REQUIRED', 'Package name and version are required (e.g., pnpm dist-tag add pkg@1.0.0 latest)')
  }

  const { name: packageName, versionRange: version } = parsePackageSpec(params[0])

  if (!version) {
    throw new PnpmError('DIST_TAG_ADD_VERSION_REQUIRED', 'Version is required (e.g., pnpm dist-tag add pkg@1.0.0 latest)')
  }

  if (!semver.valid(version)) {
    throw new PnpmError('DIST_TAG_ADD_INVALID_VERSION', `Version must be an exact semver version, got "${version}"`)
  }

  const tag = params[1] ?? 'latest'

  const registryUrl = pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)
  const authHeader = getAuthHeader(registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const distTagUrl = getDistTagUrl(packageName, registryUrl, tag)
  const response = await fetchFromRegistry(distTagUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify(version),
  })

  if (!response.ok) {
    await throwRegistryError(response, `set dist-tag "${tag}" on`)
  }

  return `+${tag}: ${packageName}@${version}`
}

async function distTagRm (
  opts: DistTagOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('DIST_TAG_RM_ARGS_REQUIRED', 'Package name and tag are required (e.g., pnpm dist-tag rm pkg tag)')
  }

  const packageName = params[0]
  const tag = params[1]

  if (tag === 'latest') {
    throw new PnpmError('DIST_TAG_RM_LATEST', 'Removing the "latest" dist-tag is not allowed')
  }

  const registryUrl = pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName)
  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri ?? {}, registryUrl)
  const authHeader = getAuthHeader(registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  // First check the tag exists
  const distTags = await fetchDistTags(packageName, registryUrl, fetchFromRegistry, authHeader)
  if (!(tag in distTags)) {
    throw new PnpmError('DIST_TAG_NOT_FOUND', `dist-tag "${tag}" is not set on package "${packageName}"`)
  }

  const distTagUrl = getDistTagUrl(packageName, registryUrl, tag)
  const response = await fetchFromRegistry(distTagUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      ...(otp ? { 'npm-otp': otp } : {}),
    },
  })

  if (!response.ok) {
    await throwRegistryError(response, `remove dist-tag "${tag}" from`)
  }

  return `-${tag}: ${packageName}@${distTags[tag]}`
}

function getDistTagUrl (packageName: string, registryUrl: string, tag: string): string {
  return new URL(`-/package/${encodeURIComponent(packageName)}/dist-tags/${encodeURIComponent(tag)}`, registryUrl).href
}

async function fetchDistTags (
  packageName: string,
  registryUrl: string,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined
): Promise<Record<string, string>> {
  const distTagsUrl = new URL(`-/package/${encodeURIComponent(packageName)}/dist-tags`, registryUrl).href
  const response = await fetchFromRegistry(distTagsUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch package info: ${response.status} ${response.statusText}`)
  }

  return await response.json() as Record<string, string>
}

async function throwRegistryError (response: Response, action: string): Promise<never> {
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${action} packages`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action} this package`)
  }
  const errorBody = await response.text()
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${errorBody}`)
}
