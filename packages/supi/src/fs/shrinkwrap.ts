import {Resolution} from '@pnpm/resolver-base'
import encodeRegistry = require('encode-registry')

export function absolutePathToRef (
  absolutePath: string,
  opts: {
    alias: string,
    realName: string,
    resolution: Resolution,
    standardRegistry: string,
  },
) {
  if (opts.resolution.type) return absolutePath

  const registryName = encodeRegistry(opts.standardRegistry)
  if (absolutePath.startsWith(`${registryName}/`) && absolutePath.indexOf('/-/') === -1) {
    if (opts.alias === opts.realName) {
      const ref = absolutePath.replace(`${registryName}/${opts.realName}/`, '')
      if (ref.indexOf('/') === -1) return ref
    }
    return absolutePath.replace(`${registryName}/`, '/')
  }
  return absolutePath
}
