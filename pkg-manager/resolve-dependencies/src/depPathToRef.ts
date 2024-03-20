import encodeRegistry from 'encode-registry'

import type { Registries, Resolution } from '@pnpm/types'
import { getRegistryByPackageName } from '@pnpm/dependency-path'

export function depPathToRef(
  depPath: string,
  opts: {
    alias: string
    realName?: string | undefined
    registries: Registries
    resolution?: Resolution | undefined
  }
): string {
  if (opts.resolution?.type) {
    return depPath
  }

  const registryName = encodeRegistry(
    getRegistryByPackageName(opts.registries, opts.realName)
  )

  if (depPath.startsWith(`${registryName}/`)) {
    depPath = depPath.replace(`${registryName}/`, '/')
  }

  if (depPath.startsWith('/') && opts.alias === opts.realName) {
    const ref = depPath.replace(`/${opts.realName}/`, '')

    if (!ref.includes('/') || !ref.replace(/(\([^)]+\))+$/, '').includes('/')) {
      return ref
    }
  }

  return depPath
}
