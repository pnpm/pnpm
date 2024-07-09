import { type CatalogSnapshots } from '@pnpm/lockfile-types'
import { type ResolvedDirectDependency } from './resolveDependencyTree'

export function getCatalogSnapshots (resolvedDirectDeps: readonly ResolvedDirectDependency[]): CatalogSnapshots {
  const catalogSnapshots: CatalogSnapshots = {}
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= {})
    snapshotForSingleCatalog[dep.alias] = {
      specifier: dep.catalogLookup.specifier,
      version: dep.version,
    }
  }

  return catalogSnapshots
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
