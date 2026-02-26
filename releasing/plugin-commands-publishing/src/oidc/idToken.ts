import { PnpmError } from '@pnpm/error'
import { displayError } from '../displayError.js'
import { type PublishPackedPkgOptions } from '../publishPackedPkg.js'
import { SHARED_CONTEXT } from './utils/shared-context.js'

export interface IdTokenDate {
  now: (this: this) => number
}

export interface IdTokenCIInfo {
  GITHUB_ACTIONS?: boolean
  GITLAB?: boolean
}

export interface IdTokenEnv extends NodeJS.ProcessEnv {
  ACTIONS_ID_TOKEN_REQUEST_TOKEN?: string
  ACTIONS_ID_TOKEN_REQUEST_URL?: string
  NPM_ID_TOKEN?: string
}

export interface IdTokenFetchOptions {
  body?: null
  headers: {
    Accept: 'application/json'
    Authorization: `Bearer ${string}`
  }
  method?: 'GET'
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface IdTokenFetchResponse {
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface IdTokenContext {
  Date: IdTokenDate
  ciInfo: IdTokenCIInfo
  fetch: (url: string, options: IdTokenFetchOptions) => Promise<IdTokenFetchResponse>
  globalInfo: (message: string) => void
  process: { env?: IdTokenEnv }
}

export type IdTokenOptions = Pick<PublishPackedPkgOptions,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
>

export interface IdTokenParams {
  context?: IdTokenContext
  options?: IdTokenOptions
  registry: string
}

/**
 * Retrieve an `idToken` from the CI environment.
 *
 * @throws instances of subclasses of {@link IdTokenError} which can be converted into warnings and skipped.
 *
 * @see https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect for GitHub Actions OIDC.
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js#L37-L110 for npm's implementation
 * @see https://github.com/yarnpkg/berry/blob/bafbef55/packages/plugin-npm/sources/npmHttpUtils.ts#L594-L624 for yarn's implementation
 */
export async function getIdToken ({
  context: {
    Date,
    ciInfo: { GITHUB_ACTIONS, GITLAB },
    fetch,
    globalInfo,
    process: { env },
  } = SHARED_CONTEXT,
  options,
  registry,
}: IdTokenParams): Promise<string | undefined> {
  if (!GITHUB_ACTIONS && !GITLAB) return undefined

  if (env?.NPM_ID_TOKEN) return env.NPM_ID_TOKEN

  if (!GITHUB_ACTIONS) return undefined

  if (!env?.ACTIONS_ID_TOKEN_REQUEST_TOKEN || !env?.ACTIONS_ID_TOKEN_REQUEST_URL) {
    throw new IdTokenGitHubWorkflowIncorrectPermissionsError()
  }

  const parsedRegistry = new URL(registry)
  const audience = `npm:${parsedRegistry.hostname}` as const
  const url = new URL(env.ACTIONS_ID_TOKEN_REQUEST_URL)
  url.searchParams.append('audience', audience)
  const startTime = Date.now()
  const response = await fetch(url.href, {
    headers: {
      Accept: 'application/json',
      Authorization: `Bearer ${env.ACTIONS_ID_TOKEN_REQUEST_TOKEN}`,
    },
    method: 'GET',
    retry: {
      factor: options?.fetchRetryFactor,
      maxTimeout: options?.fetchRetryMaxtimeout,
      minTimeout: options?.fetchRetryMintimeout,
      retries: options?.fetchRetries,
    },
    timeout: options?.fetchTimeout,
  })

  const elapsedTime = Date.now() - startTime
  globalInfo(`GET ${url.href} ${response.status} ${elapsedTime}ms`)

  if (!response.ok) {
    throw new IdTokenGitHubInvalidResponseError()
  }

  let json: unknown
  try {
    json = await response.json()
  } catch (error) {
    throw new IdTokenGitHubJsonInterruptedError(error)
  }

  if (!json || typeof json !== 'object' || !('value' in json) || typeof json.value !== 'string') {
    throw new IdTokenGitHubJsonInvalidValueError(json)
  }

  return json.value
}

export abstract class IdTokenError extends PnpmError {}

export class IdTokenGitHubWorkflowIncorrectPermissionsError extends IdTokenError {
  constructor () {
    super('ID_TOKEN_GITHUB_WORKFLOW_INCORRECT_PERMISSIONS', 'Incorrect permissions for idToken within GitHub Workflows')
  }
}

export class IdTokenGitHubInvalidResponseError extends IdTokenError {
  constructor () {
    super('ID_TOKEN_GITHUB_INVALID_RESPONSE', 'Failed to fetch idToken from GitHub: received an invalid response')
  }
}

export class IdTokenGitHubJsonInterruptedError extends IdTokenError {
  readonly errorSource: unknown
  constructor (error: unknown) {
    super('ID_TOKEN_GITHUB_JSON_INTERRUPTED_ERROR', `Fetching of idToken JSON interrupted: ${displayError(error)}`)
    this.errorSource = error
  }
}

export class IdTokenGitHubJsonInvalidValueError extends IdTokenError {
  readonly jsonResponse: unknown
  constructor (jsonResponse: unknown) {
    super('ID_TOKEN_GITHUB_JSON_INVALID_VALUE', 'Failed to fetch idToken from GitHub: missing or invalid value')
    this.jsonResponse = jsonResponse
  }
}
