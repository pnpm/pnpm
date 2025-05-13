import { type Catalogs } from '@pnpm/catalogs.types'
import { type CatalogSnapshots } from '@pnpm/lockfile.types'
import { type ResolvedDirectDependency } from './resolveDependencyTree'

export function getCatalogSnapshots (resolvedDirectDeps: readonly ResolvedDirectDependency[], updatedCatalogConfigs: Catalogs): CatalogSnapshots {
  const catalogSnapshots: CatalogSnapshots = {}
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= {})
    const updatedSpecifier = updatedCatalogConfigs[dep.catalogLookup.catalogName]?.[dep.alias]

    snapshotForSingleCatalog[dep.alias] = {
      // The "updated specifier" will be present when "pnpm update" is ran and
      // bare specifiers need to be upserted in pnpm-workspace.yaml file. When
      // this happens, save the updated specifier to the lockfile instead of the
      // original specifier.
      specifier: updatedSpecifier ?? dep.catalogLookup.specifier,
      version: dep.version,
    }
  }

  return catalogSnapshots
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
