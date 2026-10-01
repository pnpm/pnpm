import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { RegistryConfig } from '@pnpm/types'

import { normalizeRegistryUrl, readErrorBody } from './common.js'
import type { TeamOptions } from './team.js'

export function getRegistryAndAuthForOrg (
  opts: TeamOptions,
  scope: string
): { registryUrl: string, authHeader: string | undefined } {
  const pkgName = `@${scope}/__pnpm_team__`
  const registryUrl = pickRegistryForPackage(opts.registriesByScope ?? { default: 'https://registry.npmjs.org/' }, pkgName)
  const authHeader = getAuthHeaderForRegistry(opts.configByUri, registryUrl, pkgName)
  if (!authHeader) {
    throw new PnpmError('TEAM_MISSING_AUTH', 'Authentication required for registry access')
  }
  return { registryUrl, authHeader }
}

function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string,
  packageName: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {})
  return getAuthHeader(registryUrl, { pkgName: packageName })
}

export function getOrgTeamsUrl (registryUrl: string, scope: string): string {
  return new URL(`-/org/${encodeURIComponent(scope)}/team`, normalizeRegistryUrl(registryUrl)).href
}

export function getTeamUrl (registryUrl: string, scope: string, team: string): string {
  return new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}`, normalizeRegistryUrl(registryUrl)).href
}

export function getTeamMembersUrl (registryUrl: string, scope: string, team: string): string {
  return new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}/user`, normalizeRegistryUrl(registryUrl)).href
}

export async function throwRegistryError (response: Response, action: string): Promise<never> {
  const errorBody = await readErrorBody(response)
  const safeErrorBody = [...errorBody]
    .filter(char => {
      const code = char.charCodeAt(0)
      return code > 0x1f && (code < 0x7f || code > 0x9f)
    })
    .join('')
    .slice(0, 500)
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${action}. ${safeErrorBody}`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action}. ${safeErrorBody}`)
  }
  if (response.status === 404) {
    throw new PnpmError('NOT_FOUND', `Organization or team not found. ${safeErrorBody}`)
  }
  if (response.status === 409) {
    throw new PnpmError('TEAM_CONFLICT', `Team operation failed due to conflict. ${safeErrorBody}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action}: ${response.status} ${response.statusText}. ${safeErrorBody}`)
}
