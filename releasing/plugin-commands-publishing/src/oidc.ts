import ciInfo from 'ci-info'
import { PnpmError } from '@pnpm/error'
import { fetch } from '@pnpm/fetch'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { type PublishPackedPkgOptions } from './publishPackedPkg.js'

export interface CIInfo {
  GITHUB_ACTIONS?: boolean
  GITLAB?: boolean
}

export interface OidcEnv extends NodeJS.ProcessEnv {
  ACTIONS_ID_TOKEN_REQUEST_TOKEN?: string
  ACTIONS_ID_TOKEN_REQUEST_URL?: string
  NPM_ID_TOKEN?: string
  SIGSTORE_ID_TOKEN?: string
}

interface OidcFetchOptionsBase {
  body?: string | null
  headers: {
    Accept: 'application/json'
    Authorization: `Bearer ${string}`
  }
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

interface OidcFetchOptionsIdToken extends OidcFetchOptionsBase {
  body?: null
  method: 'GET'
}

interface OidcFetchOptionsAuthToken extends OidcFetchOptionsBase {
  body?: ''
  headers: OidcFetchOptionsBase['headers'] & {
    'Content-Length': '0'
  }
  method: 'POST'
}

interface OidcFetchOptionsVisibility extends OidcFetchOptionsBase {
  method: 'GET'
}

export type OidcFetchOptions =
| OidcFetchOptionsIdToken
| OidcFetchOptionsAuthToken
| OidcFetchOptionsVisibility

export interface OidcFetchResponse {
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface OidcContext {
  ciInfo: CIInfo
  fetch: (url: string, options: OidcFetchOptions) => Promise<OidcFetchResponse>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: { env?: OidcEnv }
}

const DEFAULT_OIDC_CONTEXT: OidcContext = {
  ciInfo,
  fetch,
  globalInfo,
  globalWarn,
  process,
}

export type OidcOptions = Pick<PublishPackedPkgOptions,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'provenance'
>

export interface OidcParams {
  context?: OidcContext
  options?: OidcOptions
  packageName: string
  registry: string
}

export interface OidcResult {
  token: string
  provenance: boolean | undefined
}

/**
 * Handles OpenID Connect.
 *
 * @returns an authentication token (`authToken`).
 *
 * @throws instances of subclasses of {@link OidcError} which can be converted into warnings and skipped.
 *
 * @see https://docs.github.com/en/actions/deployment/security-hardening-your-deployments/about-security-hardening-with-openid-connect for GitHub Actions OIDC.
 * @see https://api-docs.npmjs.com/#tag/OIDC/operation/exchangeOidcToken for NPM Registry OIDC.
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js for npm's implementation.
 * @see https://github.com/yarnpkg/berry/blob/bafbef55/packages/plugin-npm/sources/npmHttpUtils.ts#L593-L644 for yarn's implementation.
 */
export async function oidc ({
  context: {
    ciInfo: { GITHUB_ACTIONS, GITLAB },
    fetch,
    globalInfo,
    globalWarn,
    process: { env },
  } = DEFAULT_OIDC_CONTEXT,
  options,
  packageName,
  registry,
}: OidcParams): Promise<OidcResult | undefined> {
  if (!GITHUB_ACTIONS && !GITLAB) return undefined

  let idToken = env?.NPM_ID_TOKEN

  if (!idToken && GITHUB_ACTIONS) {
    if (!env?.ACTIONS_ID_TOKEN_REQUEST_TOKEN || !env?.ACTIONS_ID_TOKEN_REQUEST_URL) {
      throw new OidcGitHubWorkflowIncorrectPermissionsError()
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
      throw new OidcGitHubIdTokenInvalidResponseError()
    }

    const json = await response.json().catch(error => {
      throw new OidcGitHubIdTokenJsonInterruptedError(error)
    })

    if (!json || typeof json !== 'object' || !('value' in json) || typeof json.value !== 'string') {
      throw new OidcGitHubIdTokenJsonInvalidValueError(json)
    }

    idToken = json.value
  }

  if (!idToken) {
    throw new OidcIdTokenNotAvailable()
  }

  // see <https://github.com/npm/npm-package-arg/blob/0d7bd85a85fa2571fa532d2fc842ed099b236ad2/lib/npa.js#L188>
  const escapedPackageName = packageName.replace('/', '%2f')

  let authTokenResponse: OidcFetchResponse
  try {
    authTokenResponse = await fetch(
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
    throw new OidcFailedToFetchAuthToken(error, packageName, registry)
  }

  if (!authTokenResponse.ok) {
    const error = await authTokenResponse.json().catch(() => undefined)
    throw new OidcFailedAuthTokenExchangeError(error as OidcFailedAuthTokenExchangeError['errorResponse'], authTokenResponse.status)
  }

  const json = await authTokenResponse.json().catch(error => {
    throw new OidcAuthTokenJsonInterruptedError(error)
  })

  if (!json || typeof json !== 'object' || !('token' in json) || typeof json.token !== 'string') {
    throw new OidcAuthTokenMalformedJsonError(json, packageName, registry)
  }

  const result: OidcResult = {
    token: json.token,
    provenance: options?.provenance,
  }

  if (result.provenance == null) {
    globalInfo('Provenance was not set. Determining provenance...')

    const [headerB64, payloadB64] = idToken.split('.')
    if (!headerB64 || !payloadB64) {
      globalWarn('The received idToken is not a valid JWT')
      return result
    }

    try {
      interface Payload {
        repository_visibility?: unknown
        project_visibility?: unknown
      }

      const payloadJson = Buffer.from(payloadB64, 'base64').toString('utf8')
      const payload: Payload = JSON.parse(payloadJson)

      if (
        (!GITHUB_ACTIONS || payload.repository_visibility !== 'public') &&
        (!GITLAB || payload.project_visibility !== 'public' || !env?.SIGSTORE_ID_TOKEN)
      ) {
        globalWarn('The environment does not provide enough information to determine visibility. Skipping provenance.')
        return result
      }

      const visibilityUrl = new URL(`/-/package/${escapedPackageName}/visibility`, registry)
      const response = await fetch(visibilityUrl.href, {
        headers: {
          Accept: 'application/json',
          Authorization: `Bearer ${result.token}`,
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

      if (!response.ok) {
        globalWarn(`Failed to fetch visibility for package ${packageName} from registry ${registry} (status code ${response.status})`)
        return result
      }

      const visibility = await response.json() as { public?: boolean } | undefined
      if (visibility?.public) {
        globalInfo('Enabling provenance')
        result.provenance = true
        return result
      }
    } catch (error) {
      globalWarn(`Failed to determine provenance due to error: ${String(error)}`)
      return result
    }
  }

  return result
}

export abstract class OidcError extends PnpmError {}

export class OidcGitHubWorkflowIncorrectPermissionsError extends OidcError {
  constructor () {
    super('OIDC_GITHUB_WORKFLOW_INCORRECT_PERMISSIONS', 'Incorrect permissions for idToken within GitHub Workflows')
  }
}

export class OidcGitHubIdTokenInvalidResponseError extends OidcError {
  constructor () {
    super('OIDC_GITHUB_ID_TOKEN_INVALID_RESPONSE', 'Failed to fetch idToken from GitHub: received an invalid response')
  }
}

export class OidcGitHubIdTokenJsonInterruptedError extends OidcError {
  readonly errorSource: unknown
  constructor (error: unknown) {
    super('OIDC_GITHUB_ID_TOKEN_JSON_INTERRUPTED_ERROR', `Fetching of idToken JSON interrupted: ${String(error)}`)
    this.errorSource = error
  }
}

export class OidcGitHubIdTokenJsonInvalidValueError extends OidcError {
  readonly jsonResponse: unknown
  constructor (jsonResponse: unknown) {
    super('OIDC_GITHUB_ID_TOKEN_JSON_INVALID_VALUE', 'Failed to fetch idToken from GitHub: missing or invalid value')
    this.jsonResponse = jsonResponse
  }
}

export class OidcIdTokenNotAvailable extends OidcError {
  constructor () {
    super('OIDC_ID_TOKEN_NOT_AVAILABLE', 'No idToken available')
  }
}

export class OidcFailedToFetchAuthToken extends OidcError {
  readonly errorSource: unknown
  readonly packageName: string
  readonly registry: string
  constructor (error: unknown, packageName: string, registry: string) {
    super('OIDC_FAILED_TO_FETCH_AUTH_TOKEN', `Failed to fetch authToken for package ${packageName} from registry ${registry}: ${String(error)}`)
    this.errorSource = error
    this.packageName = packageName
    this.registry = registry
  }
}

export class OidcFailedAuthTokenExchangeError extends OidcError {
  readonly errorResponse?: { body?: { message?: string } }
  readonly httpStatus: number
  constructor (errorResponse: OidcFailedAuthTokenExchangeError['errorResponse'], httpStatus: number) {
    const message = errorResponse?.body?.message ?? 'Unknown error'
    super('OIDC_FAILED_AUTH_TOKEN_EXCHANGE', `Failed token exchange request with body message: ${message} (status code ${httpStatus})`)
    this.errorResponse = errorResponse
    this.httpStatus = httpStatus
  }
}

export class OidcAuthTokenJsonInterruptedError extends OidcError {
  readonly errorSource: unknown
  constructor (error: unknown) {
    super('OIDC_AUTH_TOKEN_JSON_INTERRUPTED', `Fetching of authToken JSON interrupted: ${String(error)}`)
    this.errorSource = error
  }
}

export class OidcAuthTokenMalformedJsonError extends OidcError {
  readonly malformedJsonResponse: unknown
  readonly packageName: string
  readonly registry: string
  constructor (malformedJsonResponse: unknown, packageName: string, registry: string) {
    super('OIDC_AUTH_TOKEN_MALFORMED_JSON', `Failed to fetch authToken for package ${packageName} from registry ${registry} due to malformed JSON response`)
    this.malformedJsonResponse = malformedJsonResponse
    this.packageName = packageName
    this.registry = registry
  }
}
