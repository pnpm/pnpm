import PnpmError from '@pnpm/error'
import { FetchFromRegistry } from 'fetch-from-npm-registry'
import url = require('url')
import { PackageMeta } from './pickPackage'

type RegistryResponse = {
  status: number,
  statusText: string,
  json: () => Promise<PackageMeta>,
}

// https://semver.org/#is-there-a-suggested-regular-expression-regex-to-check-a-semver-string
const semvarRegex = new RegExp(/(.*)(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/)

class RegistryResponseError extends PnpmError {
  public readonly package: string
  public readonly response: RegistryResponse
  public readonly uri: string

  constructor (opts: {
    package: string,
    response: RegistryResponse,
    uri: string,
  }) {
    let info = ''
    const matched = opts.package.match(semvarRegex)
    if (matched) {
      info = ` Did you mean ${matched[1]}?`
    }
    super(
      `REGISTRY_META_RESPONSE_${opts.response.status}`,
      `${opts.response.status} ${opts.response.statusText}: ${opts.package} (via ${opts.uri})${info}`)
    this.package = opts.package
    this.response = opts.response
    this.uri = opts.uri
  }
}

export default async function fromRegistry (
  fetch: FetchFromRegistry,
  pkgName: string,
  registry: string,
  authHeaderValue?: string
) {
  const uri = toUri(pkgName, registry)
  const response = await fetch(uri, { authHeaderValue }) as RegistryResponse
  if (response.status > 400) {
    throw new RegistryResponseError({
      package: pkgName,
      response,
      uri,
    })
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
