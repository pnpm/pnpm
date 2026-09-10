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
// children and peers. Compares dependency/peer *sets* only, not package
// identity, so callers must pass depPaths already known to share a
// `pkgIdWithPatchHash` — otherwise two unrelated leaf packages (both with empty
// sets) would count as compatible.
export function isCompatibleAndHasMoreDeps<T extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<T>,
  depPath1: DepPath,
  depPath2: DepPath,
  visited = new Set<string>()
): boolean {
  if (depPath1 === depPath2) return true
  const cycleKey = `${depPath1}->${depPath2}`
  if (visited.has(cycleKey)) return true
  visited.add(cycleKey)

  const node1 = depGraph[depPath1]
  const node2 = depGraph[depPath2]
  if (!node1 || !node2) return false
  if (nodeDepsCount(node1) < nodeDepsCount(node2)) return false

  for (const peerName of node2.resolvedPeerNames) {
    if (!node1.resolvedPeerNames.has(peerName)) return false
  }

  for (const [alias, child2DepPath] of Object.entries(node2.children!)) {
    const child1DepPath = node1.children![alias]
    if (!child1DepPath) return false
    if (child1DepPath === child2DepPath) continue

    const child1Node = depGraph[child1DepPath]
    const child2Node = depGraph[child2DepPath]
    if (
      !child1Node ||
      !child2Node ||
      child1Node.pkgIdWithPatchHash !== child2Node.pkgIdWithPatchHash ||
      !isCompatibleAndHasMoreDeps(depGraph, child1DepPath, child2DepPath, visited)
    ) {
      return false
    }
  }

  return true
}
