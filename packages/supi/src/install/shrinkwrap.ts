import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import { getRegistryByPackageName } from 'dependency-path'
import encodeRegistry = require('encode-registry')

export function absolutePathToRef (
  absolutePath: string,
  opts: {
    alias: string,
    realName: string,
    resolution: Resolution,
  } & ({ registry: string } | { registries: Registries }),
) {
  if (opts.resolution.type) return absolutePath

  const registryName = encodeRegistry(opts['registry'] || getRegistryByPackageName(opts['registries'], opts.realName))
  if (absolutePath.startsWith(`${registryName}/`) && absolutePath.indexOf('/-/') === -1) {
    if (opts.alias === opts.realName) {
      const ref = absolutePath.replace(`${registryName}/${opts.realName}/`, '')
      if (ref.indexOf('/') === -1) return ref
    }
    return absolutePath.replace(`${registryName}/`, '/')
  }
  return absolutePath
}
