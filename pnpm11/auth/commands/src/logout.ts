import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import normalizeRegistryUrl from 'normalize-registry-url'
import { readIniFile } from 'read-ini-file'
import { renderHelp } from 'render-help'
import { writeIniFile } from 'write-ini-file'

import { getRegistryConfigKey, safeReadIniFile } from './shared.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return { registry: allTypes.registry }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export const commandNames = ['logout']

export function help (): string {
  return renderHelp({
    description: 'Log out of an npm registry.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The registry to log out of',
            name: '--registry <url>',
          },
        ],
      },
    ],
    url: docsUrl('logout'),
    usages: ['pnpm logout [--registry <url>]'],
  })
}

export type LogoutCommandOptions = Pick<Config,
| 'configDir'
| 'dir'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'authConfig'
> & {
  registry?: string
}

export async function handler (
  opts: LogoutCommandOptions
): Promise<string> {
  return logout({ opts })
}

export interface LogoutFetchResponse {
  ok: boolean
  status: number
  text: () => Promise<string>
}

export interface LogoutFetchOptions {
  method?: 'DELETE'
  headers?: {
    authorization: `Bearer ${string}`
  }
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface LogoutContext {
  fetch: (url: string, options?: LogoutFetchOptions) => Promise<LogoutFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  readIniFile: (configPath: string) => Promise<object>
  writeIniFile: (configPath: string, settings: Record<string, unknown>) => Promise<void>
}

export const DEFAULT_CONTEXT: LogoutContext = {
  fetch,
  globalInfo,
  globalWarn,
  readIniFile,
  writeIniFile,
}

export interface LogoutParams {
  context?: LogoutContext
  opts: LogoutCommandOptions
}

export async function logout ({ context = DEFAULT_CONTEXT, opts }: LogoutParams): Promise<string> {
  const { globalWarn, readIniFile } = context
  const registry = normalizeRegistryUrl(opts.registry ?? 'https://registry.npmjs.org/')
  const registryConfigKey = getRegistryConfigKey(registry)
  const tokenKey = `${registryConfigKey}:_authToken`

  const token = opts.authConfig?.[tokenKey] as string | undefined

  if (!token) {
    throw new LogoutNotLoggedInError(registry)
  }

  const revokedOnRegistry = await tryRevokeToken({ context, opts, registry, token })

  const configPath = path.join(opts.configDir, 'auth.ini')
  const authIniSettings = await safeReadIniFile(readIniFile, configPath) as Record<string, unknown>

  if (tokenKey in authIniSettings) {
    await removeTokenFromAuthIni({ context, configPath, authIniSettings, tokenKey })
  } else if (revokedOnRegistry) {
    globalWarn(
      `The auth token for ${registry} was not found in ${configPath}. ` +
      'It may be configured in .npmrc or another config file. ' +
      'The token was revoked on the registry but must be removed manually from that config file.'
    )
  } else {
    throw new LogoutFailedError(registry, configPath)
  }

  return `Logged out of ${registry}`
}

interface TryRevokeTokenParams {
  context: Pick<LogoutContext, 'fetch' | 'globalInfo'>
  opts: Pick<LogoutCommandOptions, 'fetchRetries' | 'fetchRetryFactor' | 'fetchRetryMaxtimeout' | 'fetchRetryMintimeout' | 'fetchTimeout'>
  registry: string
  token: string
}

async function tryRevokeToken ({
  context: { fetch, globalInfo },
  opts,
  registry,
  token,
}: TryRevokeTokenParams): Promise<boolean> {
  const revokeUrl = new URL(`-/user/token/${encodeURIComponent(token)}`, registry).href

  try {
    const response = await fetch(revokeUrl, {
      method: 'DELETE',
      headers: {
        authorization: `Bearer ${token}`,
      },
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout,
    })

    if (!response.ok) {
      globalInfo(`Registry returned HTTP ${response.status} when revoking token`)
      return false
    }
    return true
  } catch {
    globalInfo('Could not reach the registry to revoke the token')
    return false
  }
}

interface RemoveTokenFromAuthIniParams {
  context: Pick<LogoutContext, 'writeIniFile'>
  configPath: string
  authIniSettings: Record<string, unknown>
  tokenKey: string
}

async function removeTokenFromAuthIni ({
  context: { writeIniFile },
  configPath,
  authIniSettings,
  tokenKey,
}: RemoveTokenFromAuthIniParams): Promise<void> {
  delete authIniSettings[tokenKey]
  await writeIniFile(configPath, authIniSettings)
}

class LogoutNotLoggedInError extends PnpmError {
  constructor (registry: string) {
    super('NOT_LOGGED_IN', `Not logged in to ${registry}, so can't log out`)
  }
}

class LogoutFailedError extends PnpmError {
  constructor (registry: string, configPath: string) {
    super(
      'LOGOUT_FAILED',
      `Failed to log out of ${registry}. The registry rejected the token revocation request, ` +
      `and the token was not found in ${configPath}. ` +
      'The token may be configured in .npmrc or another config file ' +
      'and must be removed manually, and may still need to be revoked on the registry.'
    )
  }
}
