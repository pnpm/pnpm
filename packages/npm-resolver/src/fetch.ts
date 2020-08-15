import {
  FetchError,
  FetchErrorRequest,
  FetchErrorResponse,
} from '@pnpm/error'
import { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching-types'
import url = require('url')
import { PackageMeta } from './pickPackage'

type RegistryResponse = {
  status: number,
  statusText: string,
  json: () => Promise<PackageMeta>,
}

// https://semver.org/#is-there-a-suggested-regular-expression-regex-to-check-a-semver-string
const semvarRegex = new RegExp(/(.*)(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/)

class RegistryResponseError extends FetchError {
  public readonly pkgName: string

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    pkgName: string
  ) {
    let hint: string | undefined
    if (response.status === 404) {
      hint = `${pkgName} is not in the npm registry.`
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
  retry: RetryTimeoutOptions,
  pkgName: string,
  registry: string,
  authHeaderValue?: string
) {
  const uri = toUri(pkgName, registry)
  const response = await fetch(uri, { authHeaderValue, retry }) as RegistryResponse
  if (response.status > 400) {
    const request = {
      authToken: authHeaderValue,
      url: uri,
    }
    throw new RegistryResponseError(request, response, pkgName)
  }
  return response.json()
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
