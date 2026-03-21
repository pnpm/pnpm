import path from 'node:path'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { generateQrCode, pollForWebAuthToken, type WebAuthFetchOptions, type WebAuthFetchResponse } from '@pnpm/network.web-auth'
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
| 'userAgent'
> & {
  registry?: string
}

export async function handler (
  opts: LoginCommandOptions
): Promise<string> {
  return login(opts)
}

export interface LoginFetchInit {
  method: string
  headers: Record<string, string>
  body?: string
}

export interface LoginFetchResponse {
  ok: boolean
  status: number
  json: () => Promise<unknown>
  text: () => Promise<string>
}

export interface LoginContext {
  Date: { now: () => number }
  setTimeout: (cb: () => void, ms: number) => void
  fetch: (url: string, init: LoginFetchInit) => Promise<LoginFetchResponse>
  prompt: (options: { message: string, name: string, type: string }) => Promise<Record<string, string>>
  process: Record<'stdin' | 'stdout', { isTTY?: boolean }>
}

async function defaultPrompt (options: { message: string, name: string, type: string }): Promise<Record<string, string>> {
  return enquirer.prompt(options as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any
}

const DEFAULT_CONTEXT: LoginContext = {
  Date,
  setTimeout: (cb, ms) => {
    globalThis.setTimeout(cb, ms)
  },
  fetch: globalThis.fetch as unknown as LoginContext['fetch'],
  prompt: defaultPrompt,
  process,
}

export async function login (
  opts: LoginCommandOptions,
  context: LoginContext = DEFAULT_CONTEXT
): Promise<string> {
  const registry = normalizeRegistryUrl(opts.registry ?? 'https://registry.npmjs.org/')

  if (!context.process.stdin.isTTY || !context.process.stdout.isTTY) {
    throw new PnpmError('LOGIN_NON_INTERACTIVE', 'The login command requires an interactive terminal')
  }

  // Try web-based login first, fall back to classic login
  let token: string
  try {
    token = await webLogin(registry, opts, context)
  } catch (err) {
    if (isWebLoginNotSupported(err)) {
      token = await classicLogin(registry, opts, context)
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
  context: LoginContext
): Promise<string> {
  const loginUrl = `${registry.replace(/\/$/, '')}/-/v1/login`

  const response = await context.fetch(loginUrl, {
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

  const webAuthContext = {
    Date: context.Date,
    setTimeout: context.setTimeout,
    fetch: context.fetch as unknown as (url: string, options: WebAuthFetchOptions) => Promise<WebAuthFetchResponse>,
  }

  return pollForWebAuthToken(body.doneUrl, webAuthContext, fetchOptions)
}

async function classicLogin (
  registry: string,
  opts: LoginCommandOptions,
  context: LoginContext
): Promise<string> {
  const { username } = await context.prompt({
    message: 'Username:',
    name: 'username',
    type: 'input',
  })
  const { password } = await context.prompt({
    message: 'Password:',
    name: 'password',
    type: 'password',
  })
  const { email } = await context.prompt({
    message: 'Email (this IS public):',
    name: 'email',
    type: 'input',
  })

  if (!username || !password || !email) {
    throw new PnpmError('LOGIN_MISSING_CREDENTIALS', 'Username, password, and email are all required')
  }

  const loginUrl = `${registry.replace(/\/$/, '')}/-/user/org.couchdb.user:${encodeURIComponent(username)}`

  const response = await context.fetch(loginUrl, {
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
  // The npmrc convention for registry-scoped config is: //<host>[:<port>]/<pathname>
  // e.g., //registry.npmjs.org/:_authToken=xxx
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
