import {Resolution} from '@pnpm/package-requester'
import encodeRegistry = require('encode-registry')

export function absolutePathToRef (
  absolutePath: string,
  opts: {
    alias: string,
    realName: string,
    resolution: Resolution,
    standardRegistry: string,
  }
) {
  if (opts.resolution.type) return absolutePath

  const registryName = encodeRegistry(opts.standardRegistry)
  if (absolutePath.startsWith(`${registryName}/`)) {
    if (opts.alias === opts.realName) {
      const ref = absolutePath.replace(`${registryName}/${opts.realName}/`, '')
      if (ref.indexOf('/') === -1) return ref
    }
    return absolutePath.replace(`${registryName}/`, '/')
  }
  return absolutePath
}
