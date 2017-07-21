import {Resolution} from 'package-store'
import encodeRegistry = require('encode-registry')

export function absolutePathToRef (
  absolutePath: string,
  pkgName: string,
  resolution: Resolution,
  standardRegistry: string
) {
  if (resolution.type) return absolutePath

  const registryName = encodeRegistry(standardRegistry)
  if (absolutePath.startsWith(`${registryName}/`)) {
    const ref = absolutePath.replace(`${registryName}/${pkgName}/`, '')
    if (ref.indexOf('/') === -1) return ref
    return absolutePath.replace(`${registryName}/`, '/')
  }
  return absolutePath
}
