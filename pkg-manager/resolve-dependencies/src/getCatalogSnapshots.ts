import { type Catalogs } from '@pnpm/catalogs.types'
import { type CatalogSnapshots } from '@pnpm/lockfile.types'
import { type ResolvedDirectDependency } from './resolveDependencyTree.js'

export function getCatalogSnapshots (
  resolvedDirectDeps: readonly ResolvedDirectDependency[],
  updatedCatalogs?: Catalogs
): CatalogSnapshots {
  const catalogSnapshots: CatalogSnapshots = {}
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= {})
    const updatedSpecifier = updatedCatalogs?.[dep.catalogLookup.catalogName]?.[dep.alias]

    snapshotForSingleCatalog[dep.alias] = {
      // The "updated specifier" will be present when pnpm add/update is ran and
      // bare specifiers need to be added in the pnpm-workspace.yaml file. When
      // this happens, the updated specifier should be saved to lockfile instead
      // of the original specifier before the update.
      specifier: updatedSpecifier ?? dep.catalogLookup.specifier,
      version: dep.version,
    }
  }

  return catalogSnapshots
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
