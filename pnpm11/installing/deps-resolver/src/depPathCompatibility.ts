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
// children and peers. Each child alias of `depPath2` must resolve either to the
// same depPath or to a variant of the same package that is itself a
// superset-or-equal, which is what lets a package whose child carries a peer
// suffix absorb the variant whose child does not.
// Compares dependency/peer *sets* only, not package identity, so callers must
// pass depPaths already known to share a `pkgIdWithPatchHash` — otherwise two
// unrelated leaf packages (both with empty sets) would count as compatible.
export function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  depPath1: DepPath,
  depPath2: DepPath
): boolean {
  // Pairs are recorded on entry, so a dependency cycle assumes compatibility
  // instead of recursing forever. The assumption is never observed by a `true`
  // result: an incompatible pair anywhere aborts the whole check.
  const visited = new Map<DepPath, Set<DepPath>>()
  const isSuperset = (supersetDepPath: DepPath, subsetDepPath: DepPath): boolean => {
    if (supersetDepPath === subsetDepPath) return true
    let subsets = visited.get(supersetDepPath)
    if (subsets == null) {
      subsets = new Set()
      visited.set(supersetDepPath, subsets)
    }
    if (subsets.has(subsetDepPath)) return true
    subsets.add(subsetDepPath)

    const supersetNode = depGraph[supersetDepPath]
    const subsetNode = depGraph[subsetDepPath]
    if (supersetNode == null || subsetNode == null) return false
    if (nodeDepsCount(supersetNode) < nodeDepsCount(subsetNode)) return false

    for (const peerName of subsetNode.resolvedPeerNames) {
      if (!supersetNode.resolvedPeerNames.has(peerName)) return false
    }

    return Object.entries(subsetNode.children!).every(([alias, subsetChildDepPath]) => {
      const supersetChildDepPath = supersetNode.children![alias]
      if (supersetChildDepPath == null) return false
      if (supersetChildDepPath === subsetChildDepPath) return true
      const supersetChildNode = depGraph[supersetChildDepPath]
      const subsetChildNode = depGraph[subsetChildDepPath]
      return supersetChildNode != null &&
        subsetChildNode != null &&
        supersetChildNode.pkgIdWithPatchHash === subsetChildNode.pkgIdWithPatchHash &&
        isSuperset(supersetChildDepPath, subsetChildDepPath)
    })
  }
  return isSuperset(depPath1, depPath2)
}
