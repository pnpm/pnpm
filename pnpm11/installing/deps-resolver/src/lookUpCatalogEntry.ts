import { type CatalogResolution, type CatalogResolver, matchCatalogResolveResult } from '@pnpm/catalogs.resolver'
import type { LockfileObject } from '@pnpm/lockfile.types'

import type { WantedDependency } from './getWantedDependencies.js'

/**
 * The catalog entry a `catalog:` specifier refers to, or `undefined` when the
 * specifier does not use the catalog protocol. A misconfigured catalog
 * reference throws.
 */
export function lookUpCatalogEntry (
  catalogResolver: CatalogResolver,
  wantedDependency: Parameters<CatalogResolver>[0]
): CatalogResolution | undefined {
  return matchCatalogResolveResult(catalogResolver(wantedDependency), {
    found: (result) => result.resolution,
    unused: () => undefined,
    misconfiguration: (result) => {
      throw result.error
    },
  })
}

export function getCatalogExistingVersionFromSnapshot (
  catalogLookup: CatalogResolution,
  wantedLockfile: LockfileObject,
  wantedDependency: WantedDependency
): string | undefined {
  if (wantedDependency.alias == null) return undefined
  const existingCatalogResolution = wantedLockfile.catalogs
    ?.[catalogLookup.catalogName]
    ?.[wantedDependency.alias]

  return existingCatalogResolution?.specifier === catalogLookup.specifier
    ? existingCatalogResolution.version
    : undefined
}
