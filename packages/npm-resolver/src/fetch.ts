import url from 'url'
import { requestRetryLogger } from '@pnpm/core-loggers'
import PnpmError, {
  FetchError,
  FetchErrorRequest,
  FetchErrorResponse,
} from '@pnpm/error'
import { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching-types'
import * as retry from '@zkochan/retry'
import { PackageMeta } from './pickPackage'

interface RegistryResponse {
  status: number
  statusText: string
  json: () => Promise<PackageMeta>
}

// https://semver.org/#is-there-a-suggested-regular-expression-regex-to-check-a-semver-string
const semverRegex = /(.*)(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/

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
      const matched = pkgName.match(semverRegex)
      if (matched != null) {
        hint += ` Did you mean ${matched[1]}?`
      }
    }
    super(request, response, hint)
    this.pkgName = pkgName
  }
}

export default async function fromRegistry (
  fetch: FetchFromRegistry,
  fetchOpts: { retry: RetryTimeoutOptions, timeout: number },
  pkgName: string,
  registry: string,
  authHeaderValue?: string
): Promise<PackageMeta> {
  const uri = toUri(pkgName, registry)
  const op = retry.operation(fetchOpts.retry)
  return new Promise((resolve, reject) =>
    op.attempt(async (attempt) => {
      let response: RegistryResponse
      try {
        response = await fetch(uri, {
          authHeaderValue,
          compress: true,
          retry: fetchOpts.retry,
          timeout: fetchOpts.timeout,
        }) as RegistryResponse
      } catch (error: any) { // eslint-disable-line
        reject(new PnpmError('META_FETCH_FAIL', `GET ${uri}: ${error.message as string}`, { attempts: attempt }))
        return
      }
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
      } catch (error: any) { // eslint-disable-line
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
          maxRetries: fetchOpts.retry.retries!,
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
    encodedName = `@${encodeURIComponent(pkgName.slice(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }

  return new url.URL(encodedName, registry.endsWith('/') ? registry : `${registry}/`).toString()
}
