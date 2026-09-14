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
// A pair reached twice is taken as compatible, which is what terminates
// dependency cycles.
export function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  depPath1: DepPath,
  depPath2: DepPath
): boolean {
  const queued = new Map<DepPath, Set<DepPath>>()
  const pending: Array<[DepPath, DepPath]> = []
  // Breadth-first, so the shallowest incompatible pair settles the check.
  const queuePair = (supersetDepPath: DepPath, subsetDepPath: DepPath): void => {
    if (supersetDepPath === subsetDepPath) return
    let subsets = queued.get(supersetDepPath)
    if (subsets == null) {
      subsets = new Set()
      queued.set(supersetDepPath, subsets)
    }
    if (subsets.has(subsetDepPath)) return
    subsets.add(subsetDepPath)
    pending.push([supersetDepPath, subsetDepPath])
  }

  queuePair(depPath1, depPath2)
  for (let cursor = 0; cursor < pending.length; cursor++) {
    const [supersetDepPath, subsetDepPath] = pending[cursor]
    const supersetNode = depGraph[supersetDepPath]
    const subsetNode = depGraph[subsetDepPath]
    if (supersetNode == null || subsetNode == null) return false
    if (nodeDepsCount(supersetNode) < nodeDepsCount(subsetNode)) return false

    for (const peerName of subsetNode.resolvedPeerNames) {
      if (!supersetNode.resolvedPeerNames.has(peerName)) return false
    }

    for (const [alias, subsetChildDepPath] of Object.entries(subsetNode.children!)) {
      const supersetChildDepPath = supersetNode.children![alias]
      if (supersetChildDepPath == null) return false
      if (supersetChildDepPath === subsetChildDepPath) continue
      const supersetChildNode = depGraph[supersetChildDepPath]
      const subsetChildNode = depGraph[subsetChildDepPath]
      if (
        supersetChildNode == null ||
        subsetChildNode == null ||
        supersetChildNode.pkgIdWithPatchHash !== subsetChildNode.pkgIdWithPatchHash
      ) {
        return false
      }
      queuePair(supersetChildDepPath, subsetChildDepPath)
    }
  }
  return true
}
