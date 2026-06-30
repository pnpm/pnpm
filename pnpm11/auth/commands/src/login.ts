import path from 'node:path'
import readline from 'node:readline'

import { input, password as passwordPrompt } from '@inquirer/prompts'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { fetch } from '@pnpm/network.fetch'
import {
  generateQrCode,
  pollForWebAuthToken,
  promptBrowserOpen,
  type PromptBrowserOpenReadlineInterface,
  SyntheticOtpError,
  type WebAuthFetchOptions,
  withOtpHandling,
} from '@pnpm/network.web-auth'
import { addUser, AddUserHttpError, AddUserNoTokenError } from '@pnpm/registry-access.client'
import normalizeRegistryUrl from 'normalize-registry-url'
import { readIniFile } from 'read-ini-file'
import { renderHelp } from 'render-help'
import { writeIniFile } from 'write-ini-file'

import { getRegistryConfigKey, safeReadIniFile } from './shared.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    registry: allTypes.registry,
    scope: allTypes.scope,
  }
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
          {
            description: 'Associate the login token with a package scope and record the scope-to-registry mapping.',
            name: '--scope <scope>',
          },
        ],
      },
    ],
    url: docsUrl('login'),
    usages: ['pnpm login [--registry <url>] [--scope <scope>]'],
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
| 'authConfig'
> & {
  registry?: string
  scope?: string
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
  input: (options: { message: string }) => Promise<string>
  password: (options: { message: string }) => Promise<string>
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
  method?: 'GET' | 'POST' | 'PUT'
  headers?: Record<string, string>
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

export interface LoginProcess {
  platform: NodeJS.Platform
  stdin: { isTTY?: boolean }
  stdout: { isTTY?: boolean }
}

export interface LoginContext {
  Date: LoginDate
  setTimeout: (cb: () => void, ms: number) => void
  createReadlineInterface: () => PromptBrowserOpenReadlineInterface
  enquirer: LoginEnquirer
  fetch: (url: string, options?: LoginFetchOptions) => Promise<LoginFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: LoginProcess
  readIniFile: (configPath: string) => Promise<object>
  writeIniFile: (configPath: string, settings: Record<string, unknown>) => Promise<void>
}

export const DEFAULT_CONTEXT: LoginContext = {
  Date,
  setTimeout,
  createReadlineInterface: readline.createInterface.bind(null, { input: process.stdin }),
  enquirer: { input, password: passwordPrompt },
  fetch,
  globalInfo,
  globalWarn,
  process,
  readIniFile,
  writeIniFile,
}

export interface LoginParams {
  context?: LoginContext
  opts: LoginCommandOptions
}

export async function login ({ context = DEFAULT_CONTEXT, opts }: LoginParams): Promise<string> {
  const {
    process,
    readIniFile,
    writeIniFile,
  } = context

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
    token = await webLogin({ context, fetchOptions, registry })
  } catch (err) {
    if (err instanceof WebLoginError && (err.httpStatus === 404 || err.httpStatus === 405)) {
      token = await classicLogin({ context, fetchOptions, registry })
    } else {
      throw err
    }
  }

  const configPath = path.join(opts.configDir, 'auth.ini')
  const settings = await safeReadIniFile(readIniFile, configPath) as Record<string, unknown>
  const registryConfigKey = getRegistryConfigKey(registry)
  const scopeKey = normalizeScope(opts.scope)
  const authConfigKey = scopeKey == null ? registryConfigKey : `${registryConfigKey}:${scopeKey}`
  settings[`${authConfigKey}:_authToken`] = token
  if (scopeKey != null) {
    settings[`${scopeKey}:registry`] = registry
  }
  await writeIniFile(configPath, settings)

  return `Logged in on ${registry}`
}

function normalizeScope (scope: string | undefined): string | undefined {
  if (scope == null) return undefined
  const trimmed = scope.trim()
  if (trimmed === '' || trimmed === '@') return undefined
  return trimmed.startsWith('@') ? trimmed : `@${trimmed}`
}

interface WebLoginParams {
  context: Pick<LoginContext, 'Date' | 'setTimeout' | 'createReadlineInterface' | 'fetch' | 'globalInfo' | 'globalWarn' | 'process'>
  fetchOptions: WebAuthFetchOptions
  registry: string
}

async function webLogin ({
  context,
  fetchOptions,
  registry,
}: WebLoginParams): Promise<string> {
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

  return promptBrowserOpen({
    authUrl: body.loginUrl,
    context,
    pollPromise,
  })
}

interface ClassicLoginParams {
  context: Pick<LoginContext, 'Date' | 'setTimeout' | 'createReadlineInterface' | 'enquirer' | 'fetch' | 'globalInfo' | 'globalWarn' | 'process'>
  fetchOptions: WebAuthFetchOptions
  registry: string
}

async function classicLogin ({
  context,
  fetchOptions,
  registry,
}: ClassicLoginParams): Promise<string> {
  const { enquirer, fetch, globalInfo, globalWarn } = context

  let username: string
  let password: string
  let email: string
  try {
    username = await enquirer.input({ message: 'Username:' })
    password = await enquirer.password({ message: 'Password:' })
    email = await enquirer.input({ message: 'Email (this IS public):' })
  } catch (err: unknown) {
    if (err instanceof Error && err.name === 'ExitPromptError') {
      throw new PnpmError('LOGIN_CANCELED', 'Login canceled')
    }
    throw err
  }

  if (!username || !password || !email) {
    throw new LoginMissingCredentialsError()
  }

  const token = await withOtpHandling({
    context,
    fetchOptions,
    operation: async (otp?: string) => {
      try {
        const result = await addUser({
          username,
          password,
          email,
          otp,
          registryUrl: registry,
          fetch,
        })
        return result.token
      } catch (err) {
        if (err instanceof AddUserHttpError) {
          if (err.status === 401 && err.responseHeaders.get('www-authenticate')?.includes('otp')) {
            throw SyntheticOtpError.fromUnknownBody(globalWarn, err.responseJson)
          }
          throw new ClassicLoginError(err.status, err.responseText)
        }
        if (err instanceof AddUserNoTokenError) {
          throw new LoginNoTokenError()
        }
        throw err
      }
    },
  })

  globalInfo(`Logged in as ${username}`)

  return token
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
