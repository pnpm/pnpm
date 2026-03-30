import { execFile } from 'node:child_process'
import path from 'node:path'
import readline from 'node:readline'
import util from 'node:util'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import {
  generateQrCode,
  offerToOpenBrowser,
  type OfferToOpenBrowserExecFile,
  type OfferToOpenBrowserReadlineInterface,
  type OfferToOpenBrowserStdin,
  pollForWebAuthToken,
  SyntheticOtpError,
  type WebAuthFetchOptions,
  withOtpHandling,
} from '@pnpm/network.web-auth'
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
            description: 'The registry to log in to',
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

export interface LoginDate {
  now: () => number
}

export interface LoginEnquirer {
  prompt: (options: LoginEnquirerOptions) => Promise<Record<string, string>>
}

export interface LoginEnquirerOptions {
  message: string
  name: string
  type: string
}

export interface LoginFetchResponse {
  ok: boolean
  status: number
  json: () => Promise<unknown>
  text: () => Promise<string>
  headers: LoginFetchResponseHeaders
}

export interface LoginFetchResponseHeaders {
  get: (name: string) => string | null
}

export interface LoginFetchOptions {
  method?: string
  headers?: {
    accept: 'application/json'
    'content-type': 'application/json'

    // Q: Why does pnpm send this header unconditionally?
    // A: This header doesn't say "I prefer web-based authentication";
    //    it only says "I am capable of web-based authentication".
    //    The npm CLI does the same:
    //    <https://github.com/npm/npm-registry-fetch/blob/844230f/lib/index.js#L196-L198>
    'npm-auth-type': 'web'

    'npm-otp'?: string
  }
  body?: string
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface LoginContext<Stdin extends { isTTY?: boolean; pause?: () => void } = OfferToOpenBrowserStdin> {
  Date: LoginDate
  setTimeout: (cb: () => void, ms: number) => void
  enquirer: LoginEnquirer
  execFile: OfferToOpenBrowserExecFile
  fetch: (url: string, options?: LoginFetchOptions) => Promise<LoginFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: {
    platform: string
    stdin: Stdin
    stdout: { isTTY?: boolean }
  }
  readline: {
    createInterface: (options: { input: Stdin }) => OfferToOpenBrowserReadlineInterface
  }
  readIniFile: (configPath: string) => Promise<object>
  writeIniFile: (configPath: string, settings: Record<string, unknown>) => Promise<void>
}

export const DEFAULT_CONTEXT: LoginContext = {
  Date,
  setTimeout,
  enquirer,
  execFile,
  fetch,
  globalInfo,
  globalWarn,
  process,
  readline,
  readIniFile,
  writeIniFile,
}

export interface LoginParams<Stdin extends { isTTY?: boolean; pause?: () => void } = OfferToOpenBrowserStdin> {
  context?: LoginContext<Stdin>
  opts: LoginCommandOptions
}

export async function login<Stdin extends { isTTY?: boolean; pause?: () => void } = OfferToOpenBrowserStdin> ({ context, opts }: LoginParams<Stdin>): Promise<string> {
  const ctx = context ?? (DEFAULT_CONTEXT as unknown as LoginContext<Stdin>)
  const {
    process,
    readIniFile,
    writeIniFile,
  } = ctx

  const registry = normalizeRegistryUrl(opts.registry ?? 'https://registry.npmjs.org/')

  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new LoginNonInteractiveError()
  }

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

  // Try web-based login first, fall back to classic login
  let token: string
  try {
    token = await webLogin({ context: ctx, fetchOptions, registry })
  } catch (err) {
    if (err instanceof WebLoginError && (err.httpStatus === 404 || err.httpStatus === 405)) {
      token = await classicLogin({ context: ctx, fetchOptions, registry })
    } else {
      throw err
    }
  }

  const configPath = path.join(opts.configDir, 'rc')
  const settings = await safeReadIniFile(readIniFile, configPath) as Record<string, unknown>
  const registryConfigKey = getRegistryConfigKey(registry)
  settings[`${registryConfigKey}:_authToken`] = token
  await writeIniFile(configPath, settings)

  return `Logged in on ${registry}`
}

interface WebLoginParams<Stdin extends { isTTY?: boolean; pause?: () => void }> {
  context: Pick<LoginContext<Stdin>, 'Date' | 'setTimeout' | 'execFile' | 'fetch' | 'globalInfo' | 'globalWarn' | 'process' | 'readline'>
  fetchOptions: WebAuthFetchOptions
  registry: string
}

async function webLogin<Stdin extends { isTTY?: boolean; pause?: () => void }> ({
  context,
  fetchOptions,
  registry,
}: WebLoginParams<Stdin>): Promise<string> {
  const {
    fetch,
    globalInfo,
  } = context

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
    throw new LoginInvalidResponseError()
  }

  const qrCode = generateQrCode(body.loginUrl)
  globalInfo(`Authenticate your account at:\n${body.loginUrl}\n\n${qrCode}`)

  const pollPromise = pollForWebAuthToken({ context, doneUrl: body.doneUrl, fetchOptions })

  return offerToOpenBrowser({
    authUrl: body.loginUrl,
    context,
    pollPromise,
  })
}

interface ClassicLoginParams<Stdin extends { isTTY?: boolean; pause?: () => void }> {
  context: Pick<LoginContext<Stdin>, 'Date' | 'setTimeout' | 'enquirer' | 'execFile' | 'fetch' | 'globalInfo' | 'globalWarn' | 'process' | 'readline'>
  fetchOptions: WebAuthFetchOptions
  registry: string
}

async function classicLogin<Stdin extends { isTTY?: boolean; pause?: () => void }> ({
  context,
  fetchOptions,
  registry,
}: ClassicLoginParams<Stdin>): Promise<string> {
  const { enquirer, fetch, globalInfo, globalWarn } = context

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
    throw new LoginMissingCredentialsError()
  }

  const loginUrl = new URL(`-/user/org.couchdb.user:${encodeURIComponent(username)}`, registry).href

  const token = await withOtpHandling({
    context,
    fetchOptions,
    operation: async (otp?: string) => {
      const response = await fetch(loginUrl, {
        method: 'PUT',
        headers: {
          'content-type': 'application/json',
          accept: 'application/json',
          'npm-auth-type': 'web',
          // Conditionally include npm-otp: some HTTP implementations coerce
          // `undefined` to the string "undefined", which would send a bad header
          // on the initial attempt (before OTP is known).
          ...(otp != null ? { 'npm-otp': otp } : {}),
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
        await throwIfOtpRequired(globalWarn, response)
        const text = await response.text()
        throw new ClassicLoginError(response.status, text)
      }

      const body = await response.json() as { token?: string }

      if (!body.token) {
        throw new LoginNoTokenError()
      }

      return body.token
    },
  })

  globalInfo(`Logged in as ${username}`)

  return token
}

/**
 * Inspects a non-ok HTTP response for OTP requirements and throws an EOTP
 * error when detected. This mirrors the behaviour of npm-registry-fetch,
 * which checks the `www-authenticate` header for one-time password indicators.
 */
async function throwIfOtpRequired (globalWarn: LoginContext['globalWarn'], response: LoginFetchResponse): Promise<void> {
  if (response.status !== 401) return

  const wwwAuth = response.headers.get('www-authenticate')
  if (!wwwAuth?.includes('otp')) return

  let body: unknown
  try {
    body = await response.json()
  } catch {}

  throw SyntheticOtpError.fromUnknownBody(globalWarn, body)
}

function getRegistryConfigKey (registryUrl: string): string {
  const url = new URL(registryUrl)
  return `//${url.host}${url.pathname}`
}

async function safeReadIniFile (
  readIniFile: LoginContext['readIniFile'],
  configPath: string
): Promise<object> {
  try {
    return await readIniFile(configPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return {}
    throw err
  }
}

class LoginNonInteractiveError extends PnpmError {
  constructor () {
    super('LOGIN_NON_INTERACTIVE', 'The login command requires an interactive terminal')
  }
}

class LoginInvalidResponseError extends PnpmError {
  constructor () {
    super('LOGIN_INVALID_RESPONSE', 'The registry returned an invalid response for web-based login')
  }
}

class LoginMissingCredentialsError extends PnpmError {
  constructor () {
    super('LOGIN_MISSING_CREDENTIALS', 'Username, password, and email are all required')
  }
}

class LoginNoTokenError extends PnpmError {
  constructor () {
    super('LOGIN_NO_TOKEN', 'The registry did not return an authentication token')
  }
}

class ClassicLoginError extends PnpmError {
  readonly httpStatus: number
  readonly responseText: string
  constructor (httpStatus: number, responseText: string) {
    super('LOGIN_FAILED', `Login failed (HTTP ${httpStatus}): ${responseText}`)
    this.httpStatus = httpStatus
    this.responseText = responseText
  }
}

class WebLoginError extends PnpmError {
  readonly httpStatus: number
  readonly responseText: string
  constructor (httpStatus: number, responseText: string) {
    super('WEB_LOGIN_FAILED', `Web-based login failed (HTTP ${httpStatus}): ${responseText}`)
    this.httpStatus = httpStatus
    this.responseText = responseText
  }
}
