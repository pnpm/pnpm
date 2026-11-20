import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { CatalogSnapshots, ProjectSnapshot } from '@pnpm/lockfile.types'

import type { ResolvedDirectDependency } from './resolveDependencyTree.js'

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

/**
 * Copies the entries of `prevCatalogs` that `importers` use into
 * `catalogSnapshots`, where the resolution recorded none. The importers are
 * the ones the resolution kept without resolving them, such as projects
 * missing from disk in a sparse checkout.
 */
export function addCatalogEntriesOfImporters (
  catalogSnapshots: CatalogSnapshots,
  importers: readonly ProjectSnapshot[],
  prevCatalogs: CatalogSnapshots | undefined
): void {
  if (prevCatalogs == null) return
  for (const importer of importers) {
    for (const [alias, specifier] of Object.entries(importer.specifiers ?? {})) {
      const catalogName = parseCatalogProtocol(specifier)
      if (catalogName == null) continue
      const entry = prevCatalogs[catalogName]?.[alias]
      if (entry == null) continue
      const snapshotForSingleCatalog = (catalogSnapshots[catalogName] ??= {})
      snapshotForSingleCatalog[alias] ??= entry
    }
  }
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
