import { docsUrl } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import type { Registries, RegistryConfig } from '@pnpm/types'
import { renderHelp } from 'render-help'

import { normalizeRegistryUrl, rcOptionsTypes, readErrorBody } from './common.js'

export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    otp: String,
    parseable: Boolean,
    json: Boolean,
  }
}

export const commandNames = ['team']

export function help (): string {
  return renderHelp({
    description: 'Manage organization teams and team memberships.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Create a new team in an organization.',
            name: 'create',
          },
          {
            description: 'Destroy an existing team.',
            name: 'destroy',
          },
          {
            description: 'Add a user to an existing team.',
            name: 'add',
          },
          {
            description: 'Remove a user from an existing team.',
            name: 'rm',
          },
          {
            description: 'List teams in an organization or users in a team.',
            name: 'ls',
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
            description: 'One-time password for registries that require two-factor authentication.',
            name: '--otp',
          },
          {
            description: 'Output parseable results.',
            name: '--parseable',
          },
          {
            description: 'Output results as JSON.',
            name: '--json',
          },
        ],
      },
    ],
    url: docsUrl('team'),
    usages: [
      'pnpm team create <scope:team> [--otp <code>]',
      'pnpm team destroy <scope:team> [--otp <code>]',
      'pnpm team add <scope:team> <user> [--otp <code>]',
      'pnpm team rm <scope:team> <user> [--otp <code>]',
      'pnpm team ls <scope|scope:team>',
    ],
  })
}

export interface TeamOptions extends CreateFetchFromRegistryOptions {
  cliOptions?: {
    otp?: string
    parseable?: boolean
    json?: boolean
  }
  configByUri?: Record<string, RegistryConfig>
  registries?: Registries
}

export async function handler (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  switch (params[0]) {
    case 'create':
      return teamCreate(opts, params.slice(1))
    case 'destroy':
      return teamDestroy(opts, params.slice(1))
    case 'add':
      return teamAdd(opts, params.slice(1))
    case 'rm':
      return teamRm(opts, params.slice(1))
    case 'ls':
    case 'list':
      return teamLs(opts, params.slice(1))
    default:
      // When no subcommand is given, assume the first arg is a scope:team
      // and list members, or a scope and list teams. This matches npm behavior
      // where `npm team ls` is the default.
      if (params.length > 0 && (params[0].startsWith('@') || params[0].startsWith(':'))) {
        return teamLs(opts, params)
      }
      throw new PnpmError('TEAM_SUBCOMMAND_REQUIRED',
        'Subcommand is required (create, destroy, add, rm, ls). Use `pnpm team ls <scope>` to list teams.')
  }
}

interface TeamInfo {
  name: string
}

interface TeamMember {
  name: string
}

async function teamCreate (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('TEAM_CREATE_SCOPE_REQUIRED',
      'Team scope is required (e.g., pnpm team create @org:newteam)')
  }

  const { scope, team } = parseScopeTeam(params[0])

  if (!team) {
    throw new PnpmError('TEAM_CREATE_NAME_REQUIRED',
      'Team name is required (e.g., pnpm team create @org:newteam)')
  }

  const { registryUrl, authHeader } = getRegistryAndAuthForOrg(opts, scope)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const teamUrl = getOrgTeamsUrl(registryUrl, scope)
  const response = await fetchFromRegistry(teamUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ name: team }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `create team "${scope}:${team}"`)
  }

  return `+${scope}:${team}`
}

async function teamDestroy (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('TEAM_DESTROY_SCOPE_REQUIRED',
      'Team scope is required (e.g., pnpm team destroy @org:newteam)')
  }

  const { scope, team } = parseScopeTeam(params[0])

  if (!team) {
    throw new PnpmError('TEAM_DESTROY_NAME_REQUIRED',
      'Team name is required (e.g., pnpm team destroy @org:newteam)')
  }

  const { registryUrl, authHeader } = getRegistryAndAuthForOrg(opts, scope)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const teamUrl = getTeamUrl(registryUrl, scope, team)
  const response = await fetchFromRegistry(teamUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      ...(otp ? { 'npm-otp': otp } : {}),
    },
  })

  if (!response.ok) {
    await throwRegistryError(response, `destroy team "${scope}:${team}"`)
  }

  return `-${scope}:${team}`
}

async function teamAdd (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('TEAM_ADD_ARGS_REQUIRED',
      'Team scope and user are required (e.g., pnpm team add @org:team username)')
  }

  const { scope, team } = parseScopeTeam(params[0])
  if (!team) {
    throw new PnpmError('TEAM_ADD_NAME_REQUIRED',
      'Team name is required (e.g., pnpm team add @org:team username)')
  }

  const username = params[1]
  const { registryUrl, authHeader } = getRegistryAndAuthForOrg(opts, scope)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const membersUrl = getTeamMembersUrl(registryUrl, scope, team)
  const response = await fetchFromRegistry(membersUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ user: username }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `add user "${username}" to team "${scope}:${team}"`)
  }

  return `+${username} added to @${scope}:${team}`
}

async function teamRm (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('TEAM_RM_ARGS_REQUIRED',
      'Team scope and user are required (e.g., pnpm team rm @org:team username)')
  }

  const { scope, team } = parseScopeTeam(params[0])
  if (!team) {
    throw new PnpmError('TEAM_RM_NAME_REQUIRED',
      'Team name is required (e.g., pnpm team rm @org:team username)')
  }

  const username = params[1]
  const { registryUrl, authHeader } = getRegistryAndAuthForOrg(opts, scope)
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const otp = opts.cliOptions?.otp

  const membersUrl = getTeamMembersUrl(registryUrl, scope, team)
  const response = await fetchFromRegistry(membersUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: {
      'content-type': 'application/json',
      ...(otp ? { 'npm-otp': otp } : {}),
    },
    body: JSON.stringify({ user: username }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `remove user "${username}" from team "${scope}:${team}"`)
  }

  return `-${username} removed from @${scope}:${team}`
}

async function teamLs (
  opts: TeamOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('TEAM_LS_SCOPE_REQUIRED',
      'Organization scope is required (e.g., pnpm team ls @org or pnpm team ls @org:team)')
  }

  const { scope, team } = parseScopeTeam(params[0])
  const { registryUrl, authHeader } = getRegistryAndAuthForOrg(opts, scope)

  const fetchFromRegistry = createFetchFromRegistry(opts)
  const parseable = opts.cliOptions?.parseable ?? false
  const json = opts.cliOptions?.json ?? false

  if (team) {
    return teamListMembers({ scope, team, registryUrl, fetchFromRegistry, authHeader, parseable, json })
  }
  return teamListTeams({ scope, registryUrl, fetchFromRegistry, authHeader, parseable, json })
}

interface TeamListOptions {
  scope: string
  registryUrl: string
  fetchFromRegistry: FetchFromRegistry
  authHeader: string | undefined
  parseable: boolean
  json: boolean
}

async function teamListTeams (options: TeamListOptions): Promise<string> {
  const { scope, registryUrl, fetchFromRegistry, authHeader, parseable, json } = options
  const teamsUrl = getOrgTeamsUrl(registryUrl, scope)
  const response = await fetchFromRegistry(teamsUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('ORG_NOT_FOUND', `Organization "@${scope}" not found in registry`)
    }
    await throwRegistryError(response, `fetch teams for "@${scope}"`)
  }

  const teams = await response.json() as TeamInfo[]

  if (json) {
    return JSON.stringify(teams.map(t => t.name), null, 2)
  }

  if (parseable) {
    return teams.map(t => t.name).join('\n')
  }

  if (teams.length === 0) {
    return `@${scope} has no teams`
  }

  const lines: string[] = [`@${scope} has the following teams:`]
  for (const { name } of teams) {
    lines.push(`  @${scope}:${name}`)
  }
  return lines.join('\n')
}

async function teamListMembers (options: TeamListOptions & { team: string }): Promise<string> {
  const { scope, team, registryUrl, fetchFromRegistry, authHeader, parseable, json } = options
  const membersUrl = getTeamMembersUrl(registryUrl, scope, team)
  const response = await fetchFromRegistry(membersUrl, {
    authHeaderValue: authHeader,
  })

  if (!response.ok) {
    if (response.status === 404) {
      throw new PnpmError('TEAM_NOT_FOUND', `Team "@${scope}:${team}" not found in registry`)
    }
    await throwRegistryError(response, `fetch team members for "@${scope}:${team}"`)
  }

  const members = await response.json() as TeamMember[]

  if (json) {
    return JSON.stringify(members.map(m => m.name), null, 2)
  }

  if (parseable) {
    return members.map(m => m.name).join('\n')
  }

  if (members.length === 0) {
    return `@${scope}:${team} has no members`
  }

  const lines: string[] = [`@${scope}:${team} has the following members:`]
  for (const { name } of members) {
    lines.push(`  ${name}`)
  }
  return lines.join('\n')
}

/**
 * Parse a scope:team string. Returns the scope (without @) and optional team name.
 * Format: @scope or @scope:team
 */
function parseScopeTeam (spec: string): { scope: string, team?: string } {
  if (!spec.startsWith('@')) {
    throw new PnpmError('TEAM_INVALID_SCOPE',
      `Team spec must start with @scope, got "${spec}". Use @scope or @scope:team format.`)
  }

  const inner = spec.slice(1)
  if (!inner) {
    throw new PnpmError('TEAM_INVALID_SCOPE',
      `Team spec must start with @scope, got "${spec}". Use @scope or @scope:team format.`)
  }

  const colonIndex = inner.indexOf(':')
  if (colonIndex === -1) {
    return { scope: inner }
  }
  const scope = inner.slice(0, colonIndex)
  const team = inner.slice(colonIndex + 1)
  if (!scope || !team) {
    throw new PnpmError('TEAM_INVALID_SCOPE',
      `Team spec must start with @scope, got "${spec}". Use @scope or @scope:team format.`)
  }
  return { scope, team }
}

function getRegistryAndAuthForOrg (
  opts: TeamOptions,
  scope: string
): { registryUrl: string, authHeader: string | undefined } {
  const pkgName = `@${scope}/__pnpm_team__`
  const registryUrl = pickRegistryForPackage(opts.registries ?? { default: 'https://registry.npmjs.org/' }, pkgName)
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

function getOrgTeamsUrl (registryUrl: string, scope: string): string {
  return new URL(`-/org/${encodeURIComponent(scope)}/team`, normalizeRegistryUrl(registryUrl)).href
}

function getTeamUrl (registryUrl: string, scope: string, team: string): string {
  return new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}`, normalizeRegistryUrl(registryUrl)).href
}

function getTeamMembersUrl (registryUrl: string, scope: string, team: string): string {
  return new URL(`-/team/${encodeURIComponent(scope)}/${encodeURIComponent(team)}/user`, normalizeRegistryUrl(registryUrl)).href
}

async function throwRegistryError (response: Response, action: string): Promise<never> {
  const errorBody = await readErrorBody(response)
  const safeErrorBody = [...errorBody]
    .filter(c => {
      const code = c.charCodeAt(0)
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
