import { type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import { getRegistryByPackageName } from '@pnpm/dependency-path'
import encodeRegistry from 'encode-registry'

export function depPathToRef (
  depPath: string,
  opts: {
    alias: string
    realName: string
    registries: Registries
    resolution: Resolution
  }
) {
  if (opts.resolution.type) return depPath

  const registryName = encodeRegistry(getRegistryByPackageName(opts.registries, opts.realName))
  if (depPath.startsWith(`${registryName}/`)) {
    depPath = depPath.replace(`${registryName}/`, '/')
  }
  if (depPath[0] === '/' && opts.alias === opts.realName) {
    const ref = depPath.replace(`/${opts.realName}@`, '')
    if (!ref.includes('/') || !ref.replace(/(\([^)]+\))+$/, '').includes('/')) return ref
  }
  return depPath
}
