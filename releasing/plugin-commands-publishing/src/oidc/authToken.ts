import { PnpmError } from '@pnpm/error'
import { displayError } from '../displayError.js'
import { type PublishPackedPkgOptions } from '../publishPackedPkg.js'
import { SHARED_CONTEXT } from './utils/shared-context.js'

export interface AuthTokenFetchOptions {
  body?: ''
  headers: {
    Accept: 'application/json'
    Authorization: `Bearer ${string}`
    'Content-Length': '0'
  }
  method?: 'POST'
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface AuthTokenFetchResponse {
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface AuthTokenContext {
  fetch: (url: string, options: AuthTokenFetchOptions) => Promise<AuthTokenFetchResponse>
}

export type AuthTokenOptions = Pick<PublishPackedPkgOptions,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
>

export interface AuthTokenParams {
  context?: AuthTokenContext
  idToken: string
  options?: AuthTokenOptions
  packageName: string
  registry: string
}

/**
 * Retrieve an `authToken` from the registry.
 *
 * @throws instances of subclasses of {@link AuthTokenError} which can be converted into warnings and skipped.
 *
 * @see https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect for GitHub Actions OIDC.
 * @see https://api-docs.npmjs.com/#tag/OIDC/operation/exchangeOidcToken for NPM Registry OIDC.
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js#L112-L142 for npm's implementation.
 * @see https://github.com/yarnpkg/berry/blob/bafbef55/packages/plugin-npm/sources/npmHttpUtils.ts#L626-L641 for yarn's implementation.
 */
export async function fetchAuthToken ({
  context: {
    fetch,
  } = SHARED_CONTEXT,
  options,
  idToken,
  packageName,
  registry,
}: AuthTokenParams): Promise<string> {
  const escapedPackageName = encodeURIComponent(packageName)

  let response: AuthTokenFetchResponse
  try {
    response = await fetch(
      new URL(`/-/npm/v1/oidc/token/exchange/package/${escapedPackageName}`, registry).href,
      {
        body: '',
        headers: {
          Accept: 'application/json',
          Authorization: `Bearer ${idToken}`,
          'Content-Length': '0',
        },
        method: 'POST',
        retry: {
          factor: options?.fetchRetryFactor,
          maxTimeout: options?.fetchRetryMaxtimeout,
          minTimeout: options?.fetchRetryMintimeout,
          retries: options?.fetchRetries,
        },
        timeout: options?.fetchTimeout,
      }
    )
  } catch (error) {
    throw new AuthTokenFetchError(error, packageName, registry)
  }

  if (!response.ok) {
    const error = await response.json().catch(() => undefined)
    throw new AuthTokenExchangeError(error as AuthTokenExchangeError['errorResponse'], response.status)
  }

  let json: unknown
  try {
    json = await response.json()
  } catch (error) {
    throw new AuthTokenJsonInterruptedError(error)
  }

  if (!json || typeof json !== 'object' || !('token' in json) || typeof json.token !== 'string') {
    throw new AuthTokenMalformedJsonError(json, packageName, registry)
  }

  return json.token
}

export abstract class AuthTokenError extends PnpmError {}

export class AuthTokenFetchError extends AuthTokenError {
  readonly errorSource: unknown
  readonly packageName: string
  readonly registry: string
  constructor (error: unknown, packageName: string, registry: string) {
    super('AUTH_TOKEN_FETCH', `Failed to fetch authToken for package ${packageName} from registry ${registry}: ${displayError(error)}`)
    this.errorSource = error
    this.packageName = packageName
    this.registry = registry
  }
}

export class AuthTokenExchangeError extends AuthTokenError {
  readonly errorResponse?: { body?: { message?: string } }
  readonly httpStatus: number
  constructor (errorResponse: AuthTokenExchangeError['errorResponse'], httpStatus: number) {
    const message = errorResponse?.body?.message ?? 'Unknown error'
    super('AUTH_TOKEN_EXCHANGE', `Failed token exchange request with body message: ${message} (status code ${httpStatus})`)
    this.errorResponse = errorResponse
    this.httpStatus = httpStatus
  }
}

export class AuthTokenJsonInterruptedError extends AuthTokenError {
  readonly errorSource: unknown
  constructor (error: unknown) {
    super('AUTH_TOKEN_JSON_INTERRUPTED', `Fetching of authToken JSON interrupted: ${displayError(error)}`)
    this.errorSource = error
  }
}

export class AuthTokenMalformedJsonError extends AuthTokenError {
  readonly malformedJsonResponse: unknown
  readonly packageName: string
  readonly registry: string
  constructor (malformedJsonResponse: unknown, packageName: string, registry: string) {
    super('AUTH_TOKEN_MALFORMED_JSON', `Failed to fetch authToken for package ${packageName} from registry ${registry} due to malformed JSON response`)
    this.malformedJsonResponse = malformedJsonResponse
    this.packageName = packageName
    this.registry = registry
  }
}
