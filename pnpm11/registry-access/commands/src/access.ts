import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

import { getStatus, listCollaborators, listPackages } from './accessList.js'
import type { AccessOptions } from './accessRequest.js'
import { grantAccess, revokeAccess, setMfa, setStatus } from './accessUpdate.js'
import { rcOptionsTypes } from './common.js'

export type { AccessOptions }
export { rcOptionsTypes }

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    json: Boolean,
    otp: String,
  }
}

export const commandNames = ['access']

const HELP_DESCRIPTION_LISTS = [
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
        description: 'One-time password for registriesByScope that require two-factor authentication.',
        name: '--otp',
      },
    ],
  },
]

export function help (): string {
  return renderHelp({
    description: 'Manages package access and visibility on the registry.',
    descriptionLists: HELP_DESCRIPTION_LISTS,
    url: docsUrl('access'),
    usages: [
      'pnpm access list packages [<user>|<scope>|<scope:team>]',
      'pnpm access list collaborators [<package> [<user>]]',
      'pnpm access get status [<package>]',
      'pnpm access set status=public|private [<package>]',
      'pnpm access set mfa=none|publish|automation [<package>]',
      'pnpm access grant <read-only|read-write> <scope:team> [<package>]',
      'pnpm access revoke <scope:team> [<package>]',
    ],
  })
}

export async function handler (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('ACCESS_SUBCOMMAND_REQUIRED', 'A subcommand is required (e.g., "list packages", "get status", "set status=public", "grant", "revoke")')
  }

  const result = dispatchSubcommand(opts, params)
  if (result != null) {
    return result
  }

  throw new PnpmError('ACCESS_UNKNOWN_SUBCOMMAND', `Unknown subcommand: ${params.join(' ')}. Run "pnpm help access" for available subcommands.`)
}

function dispatchSubcommand (opts: AccessOptions, params: string[]): Promise<string> | undefined {
  const [first, second] = params
  switch (first) {
    case 'list':
      return dispatchListSubcommand(opts, params)
    case 'ls':
      return dispatchLsSubcommand(opts, params)
    case 'get':
      return second === 'status' ? getStatus(opts, params.slice(2)) : undefined
    case 'set':
      return dispatchSetSubcommand(opts, params)
    case 'grant':
      return grantAccess(opts, params.slice(1))
    case 'revoke':
      return revokeAccess(opts, params.slice(1))
      // Handle deprecated npm access forms: public/restricted
    case 'public':
      return setStatus(opts, ['status=public', ...params.slice(1)])
    case 'restricted':
      return setStatus(opts, ['status=restricted', ...params.slice(1)])
  }
  return undefined
}

function dispatchListSubcommand (opts: AccessOptions, params: string[]): Promise<string> | undefined {
  switch (params[1]) {
    case 'packages':
      return listPackages(opts, params.slice(2))
    case 'collaborators':
      return listCollaborators(opts, params.slice(2))
  }
  return undefined
}

function dispatchLsSubcommand (opts: AccessOptions, params: string[]): Promise<string> | undefined {
  const second = params[1]
  if (second == null) {
    return listPackages(opts, params.slice(1))
  }
  if (second === 'packages') {
    return listPackages(opts, params.slice(2))
  }
  return undefined
}

function dispatchSetSubcommand (opts: AccessOptions, params: string[]): Promise<string> {
  const second = params[1]
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
