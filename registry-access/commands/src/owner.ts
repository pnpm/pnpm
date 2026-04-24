import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { normalizeRegistryUrl, rcOptionsTypes } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    otp: String,
  }
}

export const commandNames = ['owner', 'owners']

export function help (): string {
  return renderHelp({
    description: 'Manages package owners on the registry.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'List all owners of a package. Default if no subcommand is given.',
            name: 'ls',
          },
          {
            description: 'Add an owner to a package.',
            name: 'add',
          },
          {
            description: 'Remove an owner from a package.',
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
    url: docsUrl('owner'),
    usages: [
      'pnpm owner ls <package>',
      'pnpm owner add <package> <user>',
      'pnpm owner rm <package> <user>',
    ],
  })
}

export interface OwnerOptions extends CreateFetchFromRegistryOptions {
  cliOptions: {
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
  registry?: string
}

export async function handler (
  opts: OwnerOptions,
  params: string[]
): Promise<string> {
  const subcommand = params[0]

  if (subcommand === 'add') {
    return ownerAdd(opts, params.slice(1))
  }
  if (subcommand === 'rm') {
    return ownerRm(opts, params.slice(1))
  }
  if (subcommand === 'ls' || subcommand === 'list') {
    return ownerLs(opts, params.slice(1))
  }
  return ownerLs(opts, params)
}

async function ownerLs (
  opts: OwnerOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('OWNER_LS_PACKAGE_REQUIRED', 'Package name is required')
  }

  const packageName = params[0]
  const registryUrl = normalizeRegistryUrl(pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName))
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const owners = await fetchOwners(packageName, registryUrl, fetchFromRegistry, authHeader)

  const lines: string[] = []
  for (const owner of owners) {
    lines.push(`${owner.username} <${owner.email}>`)
  }
  return lines.join('\n')
}

async function ownerAdd (
  opts: OwnerOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('OWNER_ADD_ARGS_REQUIRED', 'Package name and owner are required (e.g., pnpm owner add pkg username)')
  }

  const packageName = params[0]
  const owner = params[1]

  const registryUrl = normalizeRegistryUrl(pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName))
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const ownerUrl = getOwnerUrl(packageName, registryUrl)
  const response = await fetchFromRegistry(ownerUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ user: owner }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `add owner "${owner}" to`)
  }

  return `+${owner}: ${packageName}`
}

async function ownerRm (
  opts: OwnerOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('OWNER_RM_ARGS_REQUIRED', 'Package name and owner are required (e.g., pnpm owner rm pkg username)')
  }

  const packageName = params[0]
  const owner = params[1]

  const registryUrl = normalizeRegistryUrl(pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, packageName))
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const ownerUrl = getOwnerUrl(packageName, registryUrl, owner)
  const response = await fetchFromRegistry(ownerUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      ...(otp ? { 'npm-otp': otp } : {}),
    },
  })

  if (!response.ok) {
    await throwRegistryError(response, `remove owner "${owner}" from`)
  }

  return `-${owner}: ${packageName}`
}

function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {}, registryUrl)
  return getAuthHeader(registryUrl)
}

function getOwnerUrl (packageName: string, registryUrl: string, owner?: string): string {
  const encodedName = npa(packageName).escapedName
  const base = new URL(`-/package/${encodedName}/owners`, registryUrl).href
  if (owner) {
    return `${base}/${encodeURIComponent(owner)}`
  }
  return base
}

interface Owner {
  username: string
  email: string
}

async function fetchOwners (
  packageName: string,
  registryUrl: string,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined
): Promise<Owner[]> {
  const encodedName = npa(packageName).escapedName
  const ownersUrl = new URL(`-/package/${encodedName}/owners`, registryUrl).href
  const response = await fetchFromRegistry(ownersUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    throw new PnpmError('REGISTRY_ERROR', `Failed to fetch package info: ${response.status} ${response.statusText}`)
  }

  return await response.json() as Owner[]
}

async function throwRegistryError (response: Response, action: string): Promise<never> {
  const errorBody = await response.text()
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${action} packages. ${errorBody}`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action} this package. ${errorBody}`)
  }
  if (response.status === 404) {
    throw new PnpmError('PACKAGE_NOT_FOUND', `Package not found in registry. ${errorBody}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${errorBody}`)
}