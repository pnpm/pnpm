import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { RegistriesByScope } from '@pnpm/types'

import {
  type AccessOptions,
  createPackageRequestContext,
  DEFAULT_REGISTRY_URL,
  escapePackageName,
  getAuthHeaderForRegistry,
  getRegistries,
  throwRegistryError,
} from './accessRequest.js'
import { normalizeRegistryUrl } from './common.js'

interface ListRequestContext {
  registriesByScope: RegistriesByScope
  fetchFromRegistry: FetchFromRegistry
  authHeader: string | undefined
  jsonMode: boolean
}

type EntityType = 'user' | 'org' | 'team'

export async function listPackages (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  const registriesByScope = getRegistries(opts)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registriesByScope.default ?? DEFAULT_REGISTRY_URL)
  const jsonMode = opts.cliOptions?.json ?? false
  const ctx: ListRequestContext = { registriesByScope, fetchFromRegistry, authHeader, jsonMode }

  if (params.length === 0) {
    return listOwnPackages(ctx)
  }
  const { entity, entityType } = parseEntity(params[0])
  return listEntityPackages(entityType, entity, ctx)
}

function parseEntity (raw: string): { entity: string, entityType: EntityType } {
  if (raw.includes(':')) {
    return { entityType: 'team', entity: raw }
  }
  if (raw.startsWith('@')) {
    return { entityType: 'org', entity: raw.replace(/^@/, '') }
  }
  return { entityType: 'user', entity: raw }
}

async function listOwnPackages (ctx: ListRequestContext): Promise<string> {
  const registryUrl = normalizeRegistryUrl(ctx.registriesByScope.default ?? DEFAULT_REGISTRY_URL)
  const url = new URL('-/-/package?format=cli', registryUrl).href
  return fetchListResponse(url, ctx)
}

async function listEntityPackages (
  entityType: EntityType,
  entity: string,
  ctx: ListRequestContext
): Promise<string> {
  const registryUrl = normalizeRegistryUrl(ctx.registriesByScope.default ?? DEFAULT_REGISTRY_URL)
  let listUrl: string

  if (entityType === 'team') {
    const [scope, team] = entity.split(':')
    listUrl = new URL(`-/team/${encodeURIComponent(scope.startsWith('@') ? scope.slice(1) : scope)}/${encodeURIComponent(team)}/package?format=cli`, registryUrl).href
  } else if (entityType === 'org') {
    listUrl = new URL(`-/org/${encodeURIComponent(entity)}/package?format=cli`, registryUrl).href
  } else {
    listUrl = new URL(`-/user/${encodeURIComponent(entity)}/package?format=cli`, registryUrl).href
  }

  return fetchListResponse(listUrl, ctx)
}

async function fetchListResponse (
  url: string,
  ctx: ListRequestContext
): Promise<string> {
  const response = await ctx.fetchFromRegistry(url, {
    authHeaderValue: ctx.authHeader,
  })

  if (!response.ok) {
    await throwRegistryError(response, 'list packages from')
  }

  const data = await response.json() as Record<string, unknown>
  if (ctx.jsonMode) {
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

export async function listCollaborators (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_LIST_COLLABORATORS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access list collaborators @scope/pkg)')
  }

  const packageName = params[0]
  const user = params[1]
  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
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
    await throwPackageRequestError(response, { packageName, action: 'list collaborators for' })
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

export async function getStatus (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_GET_STATUS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access get status @scope/pkg)')
  }

  const packageName = params[0]
  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
  const jsonMode = opts.cliOptions?.json ?? false

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    await throwPackageRequestError(response, { packageName, action: 'get status of' })
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

async function throwPackageRequestError (
  response: Response,
  { packageName, action }: { packageName: string, action: string }
): Promise<never> {
  if (response.status === 404) {
    throw new PnpmError('PACKAGE_NOT_FOUND', `Package "${packageName}" not found in registry`)
  }
  return throwRegistryError(response, action)
}
