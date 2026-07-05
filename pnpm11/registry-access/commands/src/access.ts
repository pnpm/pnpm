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
    json: Boolean,
    otp: String,
  }
}

export const commandNames = ['access']

export function help (): string {
  return renderHelp({
    description: 'Manages package access and visibility on the registry.',
    descriptionLists: [
      {
        title: 'Commands',

        list: [
          {
            description: 'List packages a user, scope, or team can access.',
            name: 'list packages',
          },
          {
            description: 'List collaborators on a package.',
            name: 'list collaborators',
          },
          {
            description: 'Get the public/restricted status of a package.',
            name: 'get status',
          },
          {
            description: 'Set the package visibility (public/private).',
            name: 'set status',
          },
          {
            description: 'Set the 2FA requirement for a package (none/publish/automation).',
            name: 'set mfa',
          },
          {
            description: 'Grant read-only or read-write access to a team.',
            name: 'grant',
          },
          {
            description: 'Revoke a team\'s access to a package.',
            name: 'revoke',
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
            description: 'Output results in JSON format.',
            name: '--json',
          },
          {
            description: 'One-time password for registries that require two-factor authentication.',
            name: '--otp',
          },
        ],
      },
    ],
    url: docsUrl('access'),
    usages: [
      'pnpm access list packages [<user>|<scope>|<scope:team>] [<package>]',
      'pnpm access list collaborators [<package> [<user>]]',
      'pnpm access get status [<package>]',
      'pnpm access set status=public|private [<package>]',
      'pnpm access set mfa=none|publish|automation [<package>]',
      'pnpm access grant <read-only|read-write> <scope:team> [<package>]',
      'pnpm access revoke <scope:team> [<package>]',
    ],
  })
}

export interface AccessOptions extends CreateFetchFromRegistryOptions {
  cliOptions?: {
    json?: boolean
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export async function handler (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_SUBCOMMAND_REQUIRED', 'A subcommand is required (e.g., "list packages", "get status", "set status=public", "grant", "revoke")')
  }

  const first = params[0]
  const second = params[1]

  if (first === 'list' && second === 'packages') {
    return listPackages(opts, params.slice(2))
  }
  if (first === 'list' && second === 'collaborators') {
    return listCollaborators(opts, params.slice(2))
  }
  if (first === 'ls' && (second == null || second === 'packages')) {
    return listPackages(opts, second === 'packages' ? params.slice(2) : params.slice(1))
  }
  if (first === 'get' && second === 'status') {
    return getStatus(opts, params.slice(2))
  }
  if (first === 'set') {
    if (second == null) {
      throw new PnpmError('ACCESS_SET_REQUIRED', 'A value is required (e.g., "status=public" or "mfa=none")')
    }
    if (second.startsWith('status=')) {
      return setStatus(opts, params.slice(1))
    }
    if (second.startsWith('mfa=')) {
      return setMfa(opts, params.slice(1))
    }
    throw new PnpmError('ACCESS_SET_INVALID', `Unknown set parameter "${second}". Use "status=public|private" or "mfa=none|publish|automation".`)
  }
  if (first === 'grant') {
    return grantAccess(opts, params.slice(1))
  }
  if (first === 'revoke') {
    return revokeAccess(opts, params.slice(1))
  }
  // Handle deprecated npm access forms: public/restricted
  if (first === 'public') {
    return setStatus(opts, ['status=public', ...params.slice(1)])
  }
  if (first === 'restricted') {
    return setStatus(opts, ['status=restricted', ...params.slice(1)])
  }

  throw new PnpmError('ACCESS_UNKNOWN_SUBCOMMAND', `Unknown subcommand: ${params.join(' ')}. Run "pnpm help access" for available subcommands.`)
}

async function listPackages (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registries.default ?? 'https://registry.npmjs.org/')
  const jsonMode = opts.cliOptions?.json ?? false

  let entity: string | undefined
  let entityType: 'user' | 'org' | 'team' | undefined

  if (params.length > 0) {
    const raw = params[0]
    if (raw.includes(':')) {
      entityType = 'team'
      entity = raw
    } else if (raw.startsWith('@')) {
      entityType = 'org'
      entity = raw.replace(/^@/, '')
    } else {
      entityType = 'user'
      entity = raw
    }
  }

  if (entityType == null || entity == null) {
    return listOwnPackages(params, registries, fetchFromRegistry, authHeader, jsonMode)
  }

  return listEntityPackages(entityType, entity, params.slice(1), registries, fetchFromRegistry, authHeader, jsonMode)
}

async function listOwnPackages (
  params: string[],
  registries: Registries,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined,
  jsonMode: boolean
): Promise<string> {
  const packageName = params.length > 0 ? params[0] : undefined
  if (packageName != null) {
    const registryUrl = pickRegistryForPackage(registries, packageName)
    const collaboratorsUrl = new URL(`-/package/${escapePackageName(packageName)}/collaborators?format=cli`, normalizeRegistryUrl(registryUrl)).href
    return fetchListResponse(collaboratorsUrl, registries, fetchFromRegistry, authHeader, jsonMode)
  }

  const registryUrl = normalizeRegistryUrl(registries.default ?? 'https://registry.npmjs.org/')
  const url = new URL('-/-/package?format=cli', registryUrl).href
  return fetchListResponse(url, registries, fetchFromRegistry, authHeader, jsonMode)
}

async function listEntityPackages (
  entityType: 'user' | 'org' | 'team',
  entity: string,
  params: string[],
  registries: Registries,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined,
  jsonMode: boolean
): Promise<string> {
  const registryUrl = normalizeRegistryUrl(registries.default ?? 'https://registry.npmjs.org/')
  let listUrl: string

  if (entityType === 'team') {
    const [scope, team] = entity.split(':')
    listUrl = new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}/package?format=cli`, registryUrl).href
  } else if (entityType === 'org') {
    listUrl = new URL(`-/org/${encodeURIComponent(entity)}/package?format=cli`, registryUrl).href
  } else {
    listUrl = new URL(`-/user/${encodeURIComponent(entity)}/package?format=cli`, registryUrl).href
  }

  return fetchListResponse(listUrl, registries, fetchFromRegistry, authHeader, jsonMode)
}

async function fetchListResponse (
  url: string,
  registries: Registries,
  fetchFromRegistry: FetchFromRegistry,
  authHeader: string | undefined,
  jsonMode: boolean
): Promise<string> {
  const response = await fetchFromRegistry(url, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    await throwRegistryError(response, 'list packages from')
  }

  const data = await response.json() as Record<string, unknown>
  if (jsonMode) {
    return JSON.stringify(data, null, 2)
  }
  return formatPackagesList(data)
}

function formatPackagesList (data: Record<string, unknown>): string {
  const lines: string[] = []
  for (const [pkg, access] of Object.entries(data)) {
    if (typeof access === 'string') {
      lines.push(`${pkg}: ${access}`)
    } else {
      lines.push(pkg)
    }
  }
  return lines.sort().join('\n')
}

async function listCollaborators (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_LIST_COLLABORATORS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access list collaborators @scope/pkg)')
  }

  const packageName = params[0]
  const user = params[1]
  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const jsonMode = opts.cliOptions?.json ?? false

  let collaboratorsUrl: string
  if (user) {
    collaboratorsUrl = new URL(`-/package/${escapePackageName(packageName)}/collaborators?format=cli&user=${encodeURIComponent(user)}`, normalizeRegistryUrl(registryUrl)).href
  } else {
    collaboratorsUrl = new URL(`-/package/${escapePackageName(packageName)}/collaborators?format=cli`, normalizeRegistryUrl(registryUrl)).href
  }

  const response = await fetchFromRegistry(collaboratorsUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    await throwRegistryError(response, 'list collaborators for')
  }

  const data = await response.json() as Array<Record<string, unknown>>
  if (jsonMode) {
    return JSON.stringify(data, null, 2)
  }
  return formatCollaboratorsList(data)
}

function formatCollaboratorsList (data: Array<Record<string, unknown>>): string {
  const lines: string[] = []
  for (const entry of data) {
    const user = entry.user ?? entry.username ?? 'unknown'
    const email = entry.email ?? ''
    const permissions = entry.permissions ?? 'read-only'
    lines.push(`${String(user)}${email ? ` <${email}>` : ''}: ${permissions}`)
  }
  return lines.sort().join('\n')
}

async function getStatus (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_GET_STATUS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access get status @scope/pkg)')
  }

  const packageName = params[0]
  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const jsonMode = opts.cliOptions?.json ?? false

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
    }
    await throwRegistryError(response, 'get status of')
  }

  const data = await response.json() as { access?: string, publish_requires_tfa?: unknown }
  if (jsonMode) {
    return JSON.stringify(data, null, 2)
  }

  const lines: string[] = []
  if (data.access) {
    lines.push(`package: ${packageName}`)
    lines.push(`access: ${data.access}`)
  } else {
    lines.push(`package: ${packageName}`)
    lines.push('access: public')
  }
  return lines.join('\n')
}

async function setStatus (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 || !params[0].startsWith('status=')) {
    throw new PnpmError('ACCESS_SET_STATUS_REQUIRED', 'Package visibility is required (e.g., pnpm access set status=public @scope/pkg)')
  }

  const accessValue = params[0].slice('status='.length)
  if (accessValue !== 'public' && accessValue !== 'private' && accessValue !== 'restricted') {
    throw new PnpmError('ACCESS_SET_STATUS_INVALID', `Invalid access value "${accessValue}". Must be "public" or "private".`)
  }

  const normalizedAccess = accessValue === 'private' || accessValue === 'restricted' ? 'restricted' : 'public'

  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_SET_STATUS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access set status=public @scope/pkg)')
  }

  if (!packageName.startsWith('@')) {
    throw new PnpmError('ACCESS_SET_STATUS_UNSCOPED', 'Access settings can only be changed for scoped packages (@scope/name). Unscoped packages are always public.')
  }

  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ access: normalizedAccess }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `set access to "${normalizedAccess}" for`)
  }

  return `${packageName}: ${normalizedAccess === 'public' ? 'public' : 'restricted'}`
}

async function setMfa (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 || !params[0].startsWith('mfa=')) {
    throw new PnpmError('ACCESS_SET_MFA_REQUIRED', 'MFA level is required (e.g., pnpm access set mfa=automation @scope/pkg)')
  }

  const mfaValue = params[0].slice('mfa='.length)
  if (!['none', 'publish', 'automation'].includes(mfaValue)) {
    throw new PnpmError('ACCESS_SET_MFA_INVALID', `Invalid MFA value "${mfaValue}". Must be "none", "publish", or "automation".`)
  }

  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_SET_MFA_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access set mfa=automation @scope/pkg)')
  }

  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const publishRequiresTfa = mfaValue !== 'none'

  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ publish_requires_tfa: publishRequiresTfa }),
  })

  if (!response.ok) {
    await throwRegistryError(response, 'set MFA for')
  }

  return `${packageName}: mfa=${mfaValue}`
}

async function grantAccess (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('ACCESS_GRANT_ARGS_REQUIRED', 'Permissions and scope:team are required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)')
  }

  const permissions = params[0]
  if (permissions !== 'read-only' && permissions !== 'read-write') {
    throw new PnpmError('ACCESS_GRANT_INVALID_PERMISSIONS', `Invalid permissions "${permissions}". Must be "read-only" or "read-write".`)
  }

  const scopeTeam = params[1]
  if (!scopeTeam.includes(':')) {
    throw new PnpmError('ACCESS_GRANT_INVALID_TEAM', `Invalid team "${scopeTeam}". Format must be "scope:team".`)
  }

  const packageName = params[2]
  if (!packageName) {
    throw new PnpmError('ACCESS_GRANT_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)')
  }

  const [scope, team] = scopeTeam.split(':')
  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const grantUrl = new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}/package`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(grantUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ package: packageName, permissions }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `grant ${permissions} access for ${scopeTeam} on`)
  }

  return `+${scopeTeam} (${permissions}): ${packageName}`
}

async function revokeAccess (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length < 1) {
    throw new PnpmError('ACCESS_REVOKE_ARGS_REQUIRED', 'scope:team and package name are required (e.g., pnpm access revoke @scope:developers @scope/pkg)')
  }

  const scopeTeam = params[0]
  if (!scopeTeam.includes(':')) {
    throw new PnpmError('ACCESS_REVOKE_INVALID_TEAM', `Invalid team "${scopeTeam}". Format must be "scope:team".`)
  }

  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_REVOKE_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access revoke @scope:developers @scope/pkg)')
  }

  const [scope, team] = scopeTeam.split(':')
  const registries = opts.registries ?? { default: 'https://registry.npmjs.org/' }
  const registryUrl = pickRegistryForPackage(registries, packageName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const revokeUrl = new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}/package`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(revokeUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ package: packageName }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `revoke ${scopeTeam}'s access to`)
  }

  return `-${scopeTeam}: ${packageName}`
}

function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string,
  packageName?: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {})
  return getAuthHeader(registryUrl, packageName ? { pkgName: packageName } : undefined)
}

function escapePackageName (packageName: string): string {
  const parsed = npa(packageName)
  return parsed.escapedName ?? encodeURIComponent(packageName).replace(/^%40/, '@')
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
  if (response.status === 422) {
    throw new PnpmError('ACCESS_VALIDATION_ERROR', `Invalid request: ${errorBody}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${errorBody}`)
}
