import { type CatalogSnapshots } from '@pnpm/lockfile-types'
import { type ResolvedDirectDependency } from './resolveDependencyTree'
import { type Catalogs } from '@pnpm/catalogs.types'

export function getCatalogSnapshots (
  resolvedDirectDeps: readonly ResolvedDirectDependency[],
  catalogsConfig: Catalogs,
  prevSnapshots: CatalogSnapshots = {}
): CatalogSnapshots {
  const nextSnapshots: CatalogSnapshots = {}

  // Note that "snapshotUpdates" only contains the catalog lookups relevant to a
  // filtered install. This is by design.
  //
  // To avoid wiping away previous catalog snapshots that weren't involved in a
  // filtered install, we'll need to merge these updates in with the
  // "prevSnapshots" from the existing wanted lockfile.
  const snapshotUpdates = getCatalogSnapshotsForResolvedDeps(resolvedDirectDeps)

  const catalogNames = [...Object.keys(prevSnapshots), ...Object.keys(snapshotUpdates)]
    // Avoid persisting catalog lockfile snapshots that are no longer in the
    // catalog config. This has a strange interaction with filtered installs and
    // may need to be improved. Stale catalog references may linger for an
    // importer if the catalog entry was removed but we're running a filtered
    // installed that does not include that importer.
    .filter(catalogName => catalogsConfig[catalogName] != null)

  for (const catalogName of catalogNames) {
    const prevAliases = Object.keys(prevSnapshots[catalogName] ?? {})
    const usedAliases = Object.keys(snapshotUpdates[catalogName] ?? {})

    const aliases = [...prevAliases, ...usedAliases]
      // Similar to the above, avoid persisting lockfile catalog snapshot
      // entries that are no longer part of the pnpm-workspace.yaml catalogs
      // config.
      .filter(alias => catalogsConfig[catalogName]?.[alias] != null)

    for (const alias of aliases) {
      const entry = snapshotUpdates[catalogName]?.[alias] ?? prevSnapshots[catalogName]?.[alias]
      if (entry != null) {
        (nextSnapshots[catalogName] ??= {})[alias] ??= entry
      }
    }
  }

  return nextSnapshots
}

export function getCatalogSnapshotsForResolvedDeps (resolvedDirectDeps: readonly ResolvedDirectDependency[]): CatalogSnapshots {
  const catalogSnapshots: CatalogSnapshots = {}
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= {})
    snapshotForSingleCatalog[dep.alias] = {
      specifier: dep.catalogLookup.entrySpecifier,
      version: dep.version,
    }
  }

  return catalogSnapshots
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
