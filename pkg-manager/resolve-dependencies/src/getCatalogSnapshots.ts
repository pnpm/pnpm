import { type CatalogSnapshots } from '@pnpm/lockfile-types'
import { depPathToRef } from './depPathToRef'
import { type ResolvedDirectDependency } from './resolveDependencyTree'

export function getCatalogSnapshots (resolvedDirectDeps: readonly ResolvedDirectDependency[]): CatalogSnapshots {
  const catalogSnapshots: CatalogSnapshots = {}
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= {})

    snapshotForSingleCatalog[dep.alias] = {
      specifier: dep.catalogLookup.specifier,

      // Note: The version recorded here is used for lookups of this cataloged
      // dependency in future installs. This should be computed the same as
      // other version refs in the lockfile (which are also computed through
      // depPathToRef).
      version: depPathToRef(dep.pkgId, {
        alias: dep.alias,
        realName: dep.name,
        resolution: dep.resolution,
      }),
    }
  }

  return catalogSnapshots
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
