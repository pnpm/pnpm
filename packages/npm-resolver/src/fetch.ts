import { requestRetryLogger } from '@pnpm/core-loggers'
import PnpmError, {
  FetchError,
  FetchErrorRequest,
  FetchErrorResponse,
} from '@pnpm/error'
import { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching-types'
import { PackageMeta } from './pickPackage'
import * as retry from '@zkochan/retry'
import url = require('url')

interface RegistryResponse {
  status: number
  statusText: string
  json: () => Promise<PackageMeta>
}

// https://semver.org/#is-there-a-suggested-regular-expression-regex-to-check-a-semver-string
const semvarRegex = new RegExp(/(.*)(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/)

export class RegistryResponseError extends FetchError {
  public readonly pkgName: string

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    pkgName: string
  ) {
    let hint: string | undefined
    if (response.status === 404) {
      hint = `${pkgName} is not in the npm registry, or you have no permission to fetch it.`
      const matched = pkgName.match(semvarRegex)
      if (matched) {
        hint += ` Did you mean ${matched[1]}?`
      }
    }
    super(request, response, hint)
    this.pkgName = pkgName
  }
}

export default async function fromRegistry (
  fetch: FetchFromRegistry,
  retryOpts: RetryTimeoutOptions,
  pkgName: string,
  registry: string,
  authHeaderValue?: string
): Promise<PackageMeta> {
  const uri = toUri(pkgName, registry)
  const op = retry.operation(retryOpts)
  return new Promise((resolve, reject) =>
    op.attempt(async (attempt) => {
      const response = await fetch(uri, { authHeaderValue, retry: retryOpts }) as RegistryResponse
      if (response.status > 400) {
        const request = {
          authHeaderValue,
          url: uri,
        }
        reject(new RegistryResponseError(request, response, pkgName))
        return
      }

      // Here we only retry broken JSON responses.
      // Other HTTP issues are retried by the @pnpm/fetch library
      try {
        resolve(await response.json())
      } catch (error) {
        const timeout = op.retry(
          new PnpmError('BROKEN_METADATA_JSON', error.message)
        )
        if (timeout === false) {
          reject(op.mainError())
          return
        }
        requestRetryLogger.debug({
          attempt,
          error,
          maxRetries: retryOpts.retries!,
          method: 'GET',
          timeout,
          url: uri,
        })
      }
    })
  )
}

function toUri (pkgName: string, registry: string) {
  let encodedName: string

  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.substr(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }

  return url.resolve(registry, encodedName)
}
