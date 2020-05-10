import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import { getRegistryByPackageName } from 'dependency-path'
import encodeRegistry = require('encode-registry')

export function absolutePathToRef (
  absolutePath: string,
  opts: {
    alias: string,
    realName: string,
    registries: Registries,
    resolution: Resolution,
  },
) {
  if (opts.resolution.type) return absolutePath

  const registryName = encodeRegistry(getRegistryByPackageName(opts.registries, opts.realName))
  if (absolutePath.startsWith(`${registryName}/`) && !absolutePath.includes('/-/')) {
    absolutePath = absolutePath.replace(`${registryName}/`, '/')
  }
  if (opts.alias === opts.realName) {
    const ref = absolutePath.replace(`/${opts.realName}/`, '')
    if (!ref.includes('/')) return ref
  }
  return absolutePath
}
