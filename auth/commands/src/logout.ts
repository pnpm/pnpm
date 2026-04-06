import path from 'node:path'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
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
  method?: string
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
  readIniFile: (configPath: string) => Promise<object>
  writeIniFile: (configPath: string, settings: Record<string, unknown>) => Promise<void>
}

export const DEFAULT_CONTEXT: LogoutContext = {
  fetch,
  globalInfo,
  readIniFile,
  writeIniFile,
}

export interface LogoutParams {
  context?: LogoutContext
  opts: LogoutCommandOptions
}

export async function logout ({ context = DEFAULT_CONTEXT, opts }: LogoutParams): Promise<string> {
  const {
    readIniFile,
    writeIniFile,
  } = context

  const registry = normalizeRegistryUrl(opts.registry ?? 'https://registry.npmjs.org/')
  const registryConfigKey = getRegistryConfigKey(registry)
  const tokenKey = `${registryConfigKey}:_authToken`

  // Look for the token in authConfig (merged config from all sources)
  const token = opts.authConfig?.[tokenKey] as string | undefined

  if (!token) {
    throw new LogoutNotLoggedInError(registry)
  }

  // Attempt to revoke the token on the registry
  await revokeToken({ context, opts, registry, token })

  // Remove the token from auth.ini
  const configPath = path.join(opts.configDir, 'auth.ini')
  const settings = await safeReadIniFile(readIniFile, configPath) as Record<string, unknown>
  delete settings[tokenKey]
  await writeIniFile(configPath, settings)

  return `Logged out of ${registry}`
}

interface RevokeTokenParams {
  context: Pick<LogoutContext, 'fetch' | 'globalInfo'>
  opts: Pick<LogoutCommandOptions, 'fetchRetries' | 'fetchRetryFactor' | 'fetchRetryMaxtimeout' | 'fetchRetryMintimeout' | 'fetchTimeout'>
  registry: string
  token: string
}

async function revokeToken ({
  context,
  opts,
  registry,
  token,
}: RevokeTokenParams): Promise<void> {
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
      // Token revocation is best-effort: the token will still be removed
      // locally even if the registry doesn't support revocation (404/405)
      // or the token is already invalid (401).
      globalInfo(`Registry returned HTTP ${response.status} when revoking token (token removed locally)`)
    }
  } catch {
    // Network errors during revocation should not prevent local logout
    globalInfo('Could not reach the registry to revoke the token (token removed locally)')
  }
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
