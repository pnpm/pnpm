import {Resolution} from 'package-store'
import encodeRegistry = require('encode-registry')

export function pkgIdToRef (
  pkgId: string,
  pkgName: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return pkgId

  const registryName = encodeRegistry(standardRegistry)
  if (pkgId.startsWith(`${registryName}/`)) {
    const ref = pkgId.replace(`${registryName}/${pkgName}/`, '')
    if (ref.indexOf('/') === -1) return ref
    return pkgId.replace(`${registryName}/`, '/')
  }
  return pkgId
}
