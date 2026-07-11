import type { DepPath } from '@pnpm/types'

import type {
  GenericDependenciesGraphNodeWithResolvedChildren,
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
} from './resolvePeers.js'

// Shared helpers used by both resolvePeers (dedupePeerDependents) and
// dedupeInjectedDeps. Lives in its own module so neither consumer has to import
// the other, which would create a runtime cycle. The type imports above are
// erased at build time, so no cycle exists at runtime.

export function nodeDepsCount (node: GenericDependenciesGraphNodeWithResolvedChildren): number {
  return Object.keys(node.children!).length + node.resolvedPeerNames.size
}

// Whether `depPath1` is a superset-or-equal of `depPath2`: same-or-more resolved
// children and peers. IMPORTANT: this only compares dependency/peer *sets*, not
// package identity — two different packages (or two versions of the same
// package) with compatible dependency sets, e.g. leaf nodes with none, would be
// considered compatible. Callers must therefore only compare depPaths already
// known to share the same package identity (`pkgIdWithPatchHash`). In
// `deduplicateDepPaths` that holds because the candidates are grouped by
// `pkgIdWithPatchHash`; `dedupeInjectedDeps` enforces it explicitly before
// calling this.
export function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  depPath1: DepPath,
  depPath2: DepPath
): boolean {
  const node1 = depGraph[depPath1]
  const node2 = depGraph[depPath2]
  if (nodeDepsCount(node1) < nodeDepsCount(node2)) return false

  const node1DepPathsSet = new Set(Object.values(node1.children!))
  const node2DepPaths = Object.values(node2.children!)
  if (!node2DepPaths.every((depPath) => node1DepPathsSet.has(depPath))) return false

  for (const depPath of node2.resolvedPeerNames) {
    if (!node1.resolvedPeerNames.has(depPath)) return false
  }
  return true
}
