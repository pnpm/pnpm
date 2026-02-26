import { PnpmError } from '@pnpm/error'
import { type PublishPackedPkgOptions } from '../publishPackedPkg.js'
import { SHARED_CONTEXT } from './utils/shared-context.js'

export interface ProvenanceCIInfo {
  GITHUB_ACTIONS?: boolean
  GITLAB?: boolean
}

export interface ProvenanceEnv extends NodeJS.ProcessEnv {
  SIGSTORE_ID_TOKEN?: string
}

export interface ProvenanceFetchOptions {
  headers: {
    Accept: 'application/json'
    Authorization: `Bearer ${string}`
  }
  method: 'GET'
  retry?: {
    factor?: number
    maxTimeout?: number
    minTimeout?: number
    randomize?: boolean
    retries?: number
  }
  timeout?: number
}

export interface ProvenanceFetchResponse {
  readonly json: (this: this) => Promise<unknown>
  readonly ok: boolean
  readonly status: number
}

export interface ProvenanceContext {
  ciInfo: ProvenanceCIInfo
  fetch: (url: URL, options: ProvenanceFetchOptions) => Promise<ProvenanceFetchResponse>
  process: { env?: ProvenanceEnv }
}

export type ProvenanceOptions = Pick<PublishPackedPkgOptions,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
>

export interface ProvenanceParams {
  authToken: string
  context?: ProvenanceContext
  idToken: string
  options?: ProvenanceOptions,
  packageName: string
  registry: string
}

/**
 * Determine `provenance` for a package from the CI context and the visibility of the package.
 *
 * @throws instances of subclasses of {@link ProvenanceError} which can be converted into warnings and skipped.
 *
 * @see https://github.com/npm/cli/blob/7d900c46/lib/utils/oidc.js#L145-L164 for npm's implementation.
 */
export async function determineProvenance ({
  authToken,
  idToken,
  options,
  packageName,
  registry,
  context: {
    ciInfo: { GITHUB_ACTIONS, GITLAB },
    fetch,
    process: { env },
  } = SHARED_CONTEXT,
}: ProvenanceParams): Promise<boolean | undefined> {

  const [headerB64, payloadB64] = idToken.split('.')
  if (!headerB64 || !payloadB64) {
    throw new ProvenanceMalformedIdTokenError(idToken)
  }

  interface Payload {
    repository_visibility?: unknown
    project_visibility?: unknown
  }

  const payloadJson = Buffer.from(payloadB64, 'base64url').toString('utf8')
  const payload: Payload = JSON.parse(payloadJson)

  if (
    (!GITHUB_ACTIONS || payload.repository_visibility !== 'public') &&
    (!GITLAB || payload.project_visibility !== 'public' || !env?.SIGSTORE_ID_TOKEN)
  ) {
    throw new ProvenanceInsufficientInformationError()
  }

  const escapedPackageName = encodeURIComponent(packageName)
  const visibilityUrl = new URL(`/-/package/${escapedPackageName}/visibility`, registry)
  const response = await fetch(visibilityUrl, {
    headers: {
      Accept: 'application/json',
      Authorization: `Bearer ${authToken}`,
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
    throw await ProvenanceFailedToFetchVisibilityError.createErrorFromFetchResponse(response, packageName, registry)
  }

  const visibility = await response.json() as { public?: boolean } | undefined
  if (visibility?.public) return true

  return undefined
}

export abstract class ProvenanceError extends PnpmError {}

export class ProvenanceMalformedIdTokenError extends ProvenanceError {
  readonly idToken: string
  constructor (idToken: string) {
    super('PROVENANCE_MALFORMED_ID_TOKEN', 'The received idToken is not a valid JWT')
    this.idToken = idToken
  }
}

export class ProvenanceInsufficientInformationError extends ProvenanceError {
  constructor () {
    super('PROVENANCE_INSUFFICIENT_INFORMATION', 'The environment does not provide enough information to determine visibility')
  }
}

export class ProvenanceFailedToFetchVisibilityError extends ProvenanceError {
  readonly errorResponse?: { code?: string, message?: string }
  readonly packageName: string
  readonly registry: string
  readonly status: number

  constructor (
    errorResponse: ProvenanceFailedToFetchVisibilityError['errorResponse'],
    status: number,
    packageName: string,
    registry: string
  ) {
    let message = 'an unknown error'
    if (errorResponse?.code && errorResponse?.message) {
      message = `${errorResponse.code}: ${errorResponse.message}`
    } else if (errorResponse?.code) {
      message = errorResponse.code
    } else if (errorResponse?.message) {
      message = errorResponse.message
    }
    super(
      'PROVENANCE_FAILED_TO_FETCH_VISIBILITY',
      `Failed to fetch visibility for package ${packageName} from registry ${registry} due to ${message} (status code ${status})`
    )
    this.errorResponse = errorResponse
    this.status = status
    this.packageName = packageName
    this.registry = registry
  }

  static async createErrorFromFetchResponse (response: ProvenanceFetchResponse, packageName: string, registry: string): Promise<ProvenanceFailedToFetchVisibilityError> {
    let errorResponse: ProvenanceFailedToFetchVisibilityError['errorResponse']
    try {
      errorResponse = await response.json() as typeof errorResponse
    } catch {}
    return new ProvenanceFailedToFetchVisibilityError(errorResponse, response.status, packageName, registry)
  }
}
