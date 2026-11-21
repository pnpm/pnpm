import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { CatalogSnapshots, LockfileObject, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.types'

import type { DependenciesGraph, DependenciesGraphNode } from './index.js'
import type { ResolvedDirectDependency, ResolvedImporters } from './resolveDependencyTree.js'

export function getCatalogSnapshots (
  resolvedDirectDeps: readonly ResolvedDirectDependency[],
  { graph, catalogs, updatedCatalogs }: { graph: DependenciesGraph, catalogs?: Catalogs, updatedCatalogs?: Catalogs }
): CatalogSnapshots {
  const catalogSnapshots = directoryPeerCatalogs(graph, catalogs)
  const catalogedDeps = resolvedDirectDeps.filter(isCatalogedDep)

  for (const dep of catalogedDeps) {
    const snapshotForSingleCatalog = (catalogSnapshots[dep.catalogLookup.catalogName] ??= Object.create(null) as CatalogSnapshots[string])
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

function directoryPeerCatalogs (graph: DependenciesGraph, catalogs: Catalogs = {}): CatalogSnapshots {
  const snapshots: CatalogSnapshots = Object.create(null)
  if (Object.keys(catalogs).length === 0) return snapshots
  for (const node of Object.values(graph) as DependenciesGraphNode[]) {
    if (node.resolution.type !== 'directory') continue
    for (const { name, alias, specifier, version } of peerCatalogEntries(node, { graph, catalogs })) {
      const entries = snapshots[name] ??= Object.create(null) as CatalogSnapshots[string]
      if (entries[alias] == null || version < entries[alias].version) entries[alias] = { specifier, version }
    }
  }
  return snapshots
}

function * peerCatalogEntries (node: DependenciesGraphNode, { graph, catalogs }: { graph: DependenciesGraph, catalogs: Catalogs }) {
  for (const [alias, peer] of Object.entries(node.peers.declared)) {
    const name = parseCatalogProtocol(peer.version)
    if (name == null || !Object.hasOwn(node.peers.taken, alias)) continue
    const specifier = catalogs[name]?.[alias]
    if (specifier == null) continue
    yield { name, alias, specifier, version: graph[node.children[alias]]?.version ?? specifier }
  }
}

/** Keep the catalog inputs of importers and packages reused from the previous lockfile. */
export function retainCatalogEntries (
  lockfile: LockfileObject,
  prevCatalogs: CatalogSnapshots | undefined,
  resolved: { graph: DependenciesGraph, importers: ResolvedImporters }
): void {
  if (prevCatalogs == null || lockfile.catalogs == null) return
  const importers = Object.entries(lockfile.importers)
    .filter(([id]) => !Object.hasOwn(resolved.importers, id))
    .map(([, importer]) => importer)
  const packages = Object.entries(lockfile.packages ?? {})
    .filter(([path]) => !Object.hasOwn(resolved.graph, path))
    .map(([, pkg]) => pkg)
  copyCatalogEntries(lockfile.catalogs, prevCatalogs, importerCatalogReferences(importers))
  copyCatalogEntries(lockfile.catalogs, prevCatalogs, directoryPeerCatalogReferences(packages))
}

export function copyCatalogEntries (
  catalogs: CatalogSnapshots,
  previous: CatalogSnapshots,
  references: Iterable<[string, string]>
): void {
  for (const [catalogName, alias] of references) {
    const entry = previous[catalogName]?.[alias]
    if (entry == null) continue
    const catalog = catalogs[catalogName] ??= Object.create(null) as CatalogSnapshots[string]
    catalog[alias] ??= entry
  }
}

function * importerCatalogReferences (importers: readonly ProjectSnapshot[]): Generator<[string, string]> {
  for (const importer of importers) {
    for (const [alias, specifier] of Object.entries(importer.specifiers)) {
      const name = parseCatalogProtocol(specifier)
      if (name != null) yield [name, alias]
    }
  }
}

export function * directoryPeerCatalogReferences (packages: Iterable<PackageSnapshot>): Generator<[string, string]> {
  for (const pkg of packages) {
    if (!('type' in pkg.resolution) || pkg.resolution.type !== 'directory') continue
    for (const [alias, specifier] of Object.entries(pkg.peerDependencies ?? {})) {
      const name = parseCatalogProtocol(specifier)
      if (name != null) yield [name, alias]
    }
  }
}

function isCatalogedDep (dep: ResolvedDirectDependency): dep is ResolvedDirectDependency & { catalogLookup: Required<ResolvedDirectDependency>['catalogLookup'] } {
  return dep.catalogLookup != null
}
