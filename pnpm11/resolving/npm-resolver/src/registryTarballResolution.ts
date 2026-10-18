import type { PackageInRegistry } from '@pnpm/resolving.registry.types'
import type { TarballResolution } from '@pnpm/resolving.resolver-base'
import { createRegistryRevisionsUnsupportedError } from '@pnpm/resolving.tarball-url'

import { getIntegrity } from './getIntegrity.js'
import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'

export function createRegistryTarballResolution (pickedPackage: PackageInRegistry): TarballResolution {
  const { dist } = pickedPackage
  if (dist.revision != null || dist.revisions != null) {
    throw createRegistryRevisionsUnsupportedError(
      `Cannot install ${pickedPackage.name}@${pickedPackage.version}: the registry serves it as a registry revision.`)
  }
  return {
    integrity: getIntegrity(dist),
    tarball: normalizeRegistryUrl(dist.tarball),
  }
}
