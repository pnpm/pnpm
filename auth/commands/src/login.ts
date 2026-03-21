import path from 'node:path'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import { generateQrCode, pollForWebAuthToken, type WebAuthFetchOptions } from '@pnpm/network.web-auth'
import enquirer from 'enquirer'
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

export const commandNames = ['login', 'adduser']

export function help (): string {
  return renderHelp({
    description: 'Log in to an npm registry.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The registry to log in to (default: the configured registry)',
            name: '--registry <url>',
          },
        ],
      },
    ],
    url: docsUrl('login'),
    usages: ['pnpm login [--registry <url>]'],
  })
}

export type LoginCommandOptions = Pick<Config,
| 'configDir'
| 'dir'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'rawConfig'
> & {
  registry?: string
}

export async function handler (
  opts: LoginCommandOptions
): Promise<string> {
  return login({ opts })
}

export interface LoginContext {
  Date: { now: () => number }
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: { prompt: (options: { message: string, name: string, type: string }) => Promise<Record<string, string>> }
  fetch: typeof fetch
  globalInfo: (message: string) => void
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
}

export const DEFAULT_CONTEXT: LoginContext = {
  Date,
  setTimeout,
  enquirer,
  fetch,
  globalInfo,
  process,
}

export interface LoginParams {
  context?: LoginContext
  opts: LoginCommandOptions
}

export async function login ({
  context: {
    Date,
    setTimeout,
    enquirer,
    fetch,
    globalInfo,
    process,
  } = DEFAULT_CONTEXT,
  opts,
}: LoginParams): Promise<string> {
  const registry = normalizeRegistryUrl(opts.registry ?? 'https://registry.npmjs.org/')

  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new PnpmError('LOGIN_NON_INTERACTIVE', 'The login command requires an interactive terminal')
  }

  // Try web-based login first, fall back to classic login
  let token: string
  try {
    token = await webLogin(registry, opts, { Date, setTimeout, fetch, globalInfo })
  } catch (err) {
    if (isWebLoginNotSupported(err)) {
      token = await classicLogin(registry, { enquirer, fetch, globalInfo })
    } else {
      throw err
    }
  }

  await saveToken(registry, token, opts)

  return `Logged in as ... on ${registry}`
}

async function webLogin (
  registry: string,
  opts: LoginCommandOptions,
  { Date, setTimeout, fetch, globalInfo }: Pick<LoginContext, 'Date' | 'setTimeout' | 'fetch' | 'globalInfo'>
): Promise<string> {
  const loginUrl = new URL('-/v1/login', registry).href

  const response = await fetch(loginUrl, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      accept: 'application/json',
      'npm-auth-type': 'web',
    },
    body: JSON.stringify({}),
  })

  if (!response.ok) {
    const text = await response.text()
    throw new WebLoginError(response.status, text)
  }

  const body = await response.json() as { loginUrl?: string, doneUrl?: string }

  if (!body.loginUrl || !body.doneUrl) {
    throw new PnpmError('LOGIN_INVALID_RESPONSE', 'The registry returned an invalid response for web-based login')
  }

  const qrCode = generateQrCode(body.loginUrl)
  globalInfo(`Authenticate your account at:\n${body.loginUrl}\n\n${qrCode}`)

  const fetchOptions: WebAuthFetchOptions = {
    method: 'GET',
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  }

  return pollForWebAuthToken(body.doneUrl, { Date, setTimeout, fetch }, fetchOptions)
}

async function classicLogin (
  registry: string,
  { enquirer, fetch, globalInfo }: Pick<LoginContext, 'enquirer' | 'fetch' | 'globalInfo'>
): Promise<string> {
  const { username } = await enquirer.prompt({
    message: 'Username:',
    name: 'username',
    type: 'input',
  })
  const { password } = await enquirer.prompt({
    message: 'Password:',
    name: 'password',
    type: 'password',
  })
  const { email } = await enquirer.prompt({
    message: 'Email (this IS public):',
    name: 'email',
    type: 'input',
  })

  if (!username || !password || !email) {
    throw new PnpmError('LOGIN_MISSING_CREDENTIALS', 'Username, password, and email are all required')
  }

  const loginUrl = new URL(`-/user/org.couchdb.user:${encodeURIComponent(username)}`, registry).href

  const response = await fetch(loginUrl, {
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      accept: 'application/json',
    },
    body: JSON.stringify({
      _id: `org.couchdb.user:${username}`,
      name: username,
      password,
      email,
      type: 'user',
    }),
  })

  if (!response.ok) {
    const text = await response.text()
    throw new PnpmError(
      'LOGIN_FAILED',
      `Login failed (HTTP ${response.status}): ${text}`
    )
  }

  const body = await response.json() as { token?: string, ok?: boolean }

  if (!body.token) {
    throw new PnpmError('LOGIN_NO_TOKEN', 'The registry did not return an authentication token')
  }

  globalInfo(`Logged in as ${username}`)

  return body.token
}

function getRegistryConfigKey (registryUrl: string): string {
  const url = new URL(registryUrl)
  return `//${url.host}${url.pathname}`
}

async function saveToken (
  registryUrl: string,
  token: string,
  opts: LoginCommandOptions
): Promise<void> {
  const configPath = path.join(opts.configDir, 'rc')
  const settings = await safeReadIniFile(configPath)

  const registryConfigKey = getRegistryConfigKey(registryUrl)
  settings[`${registryConfigKey}:_authToken`] = token

  await writeIniFile(configPath, settings)
}

async function safeReadIniFile (configPath: string): Promise<Record<string, unknown>> {
  try {
    return await readIniFile(configPath) as Record<string, unknown>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}

function isWebLoginNotSupported (err: unknown): boolean {
  return err instanceof WebLoginError && (err.statusCode === 404 || err.statusCode === 405)
}

class WebLoginError extends PnpmError {
  readonly statusCode: number
  constructor (statusCode: number, responseText: string) {
    super('WEB_LOGIN_FAILED', `Web-based login failed (HTTP ${statusCode}): ${responseText}`)
    this.statusCode = statusCode
  }
}
