import PnpmError from '@pnpm/error'
import url = require('url')
import { PackageMeta } from './pickPackage'

type RegistryResponse = {
  status: number,
  statusText: string,
  json: () => Promise<PackageMeta>,
}

class RegistryResponseError extends PnpmError {
  public readonly package: string
  public readonly response: RegistryResponse
  public readonly uri: string

  constructor (opts: {
    package: string,
    response: RegistryResponse,
    uri: string,
  }) {
    super(
      `REGISTRY_META_RESPONSE_${opts.response.status}`,
      `${opts.response.status} ${opts.response.statusText}: ${opts.package} (via ${opts.uri})`)
    this.package = opts.package
    this.response = opts.response
    this.uri = opts.uri
  }
}

export default async function fromRegistry (
  fetch: (url: string, opts: {auth?: object}) => Promise<{}>,
  pkgName: string,
  registry: string,
  auth?: object,
) {
  const uri = toUri(pkgName, registry)
  const response = await fetch(uri, { auth }) as RegistryResponse
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
