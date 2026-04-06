import path from 'node:path'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import normalizeRegistryUrl from 'normalize-registry-url'
import { readIniFile } from 'read-ini-file'
import { renderHelp } from 'render-help'
import { writeIniFile } from 'write-ini-file'

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

  await tryRevokeToken({ context, opts, registry, token })

  const configPath = path.join(opts.configDir, 'auth.ini')
  const authIniSettings = await safeReadIniFile(readIniFile, configPath) as Record<string, unknown>

  if (tokenKey in authIniSettings) {
    await removeTokenFromAuthIni({ context, configPath, authIniSettings, tokenKey })
  } else {
    globalWarn(
      `The auth token for ${registry} was not found in ${configPath}. ` +
      'It may be configured in .npmrc or another config file. ' +
      'The token was revoked on the registry but must be removed manually from the config file.'
    )
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
  context,
  opts,
  registry,
  token,
}: TryRevokeTokenParams): Promise<void> {
  const { fetch, globalInfo } = context

  const revokeUrl = new URL(`-/user/token/${encodeURIComponent(token)}`, registry).href

  try {
    const response = await fetch(revokeUrl, {
      method: 'DELETE',
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout,
    })

    if (!response.ok) {
      globalInfo(`Registry returned HTTP ${response.status} when revoking token (token removed locally)`)
    }
  } catch {
    globalInfo('Could not reach the registry to revoke the token (token removed locally)')
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

function getRegistryConfigKey (registryUrl: string): string {
  const url = new URL(registryUrl)
  return `//${url.host}${url.pathname}`
}

async function safeReadIniFile (
  readIniFile: LogoutContext['readIniFile'],
  configPath: string
): Promise<object> {
  try {
    return await readIniFile(configPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}

class LogoutNotLoggedInError extends PnpmError {
  constructor (registry: string) {
    super('NOT_LOGGED_IN', `Not logged in to ${registry}, so can't log out`)
  }
}
