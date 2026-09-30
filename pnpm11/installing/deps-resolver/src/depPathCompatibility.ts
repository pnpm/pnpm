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
export function isCompatibleAndHasMoreDeps<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  depPath1: DepPath,
  depPath2: DepPath
): boolean {
  const queue = createDepPathPairQueue()
  queue.push(depPath1, depPath2)
  // Breadth-first, so the shallowest incompatible pair settles the check.
  for (let cursor = 0; cursor < queue.pending.length; cursor++) {
    const [supersetDepPath, subsetDepPath] = queue.pending[cursor]
    if (!isPairCompatible(depGraph, { supersetDepPath, subsetDepPath, queue })) return false
  }
  return true
}

interface DepPathPairQueue {
  pending: Array<[DepPath, DepPath]>
  push: (supersetDepPath: DepPath, subsetDepPath: DepPath) => void
}

function createDepPathPairQueue (): DepPathPairQueue {
  const queued = new Map<DepPath, Set<DepPath>>()
  const pending: Array<[DepPath, DepPath]> = []
  const push = (supersetDepPath: DepPath, subsetDepPath: DepPath): void => {
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
  return { pending, push }
}

/**
 * Checks one pair's own dependency and peer sets and queues the pairs of
 * differing children that still have to be compared.
 */
function isPairCompatible<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  pair: { supersetDepPath: DepPath, subsetDepPath: DepPath, queue: DepPathPairQueue }
): boolean {
  const supersetNode = depGraph[pair.supersetDepPath]
  const subsetNode = depGraph[pair.subsetDepPath]
  if (supersetNode == null || subsetNode == null) return false
  if (nodeDepsCount(supersetNode) < nodeDepsCount(subsetNode)) return false

  if (!isSupersetOf(supersetNode.resolvedPeerNames, subsetNode.resolvedPeerNames)) return false
  return queueDifferingChildren(depGraph, { supersetNode, subsetNode, queue: pair.queue })
}

/**
 * Queues each child pair that resolves to different variants of the same
 * package. Returns false when a child of the subset is missing from the
 * superset or resolves to another package there.
 */
function queueDifferingChildren<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  { supersetNode, subsetNode, queue }: {
    supersetNode: GenericDependenciesGraphNodeWithResolvedChildren
    subsetNode: GenericDependenciesGraphNodeWithResolvedChildren
    queue: DepPathPairQueue
  }
): boolean {
  for (const [alias, subsetChildDepPath] of Object.entries(subsetNode.children!)) {
    const supersetChildDepPath = supersetNode.children![alias]
    if (supersetChildDepPath == null) return false
    if (supersetChildDepPath === subsetChildDepPath) continue
    if (!isSamePackage(depGraph, supersetChildDepPath, subsetChildDepPath)) return false
    queue.push(supersetChildDepPath, subsetChildDepPath)
  }
  return true
}

function isSupersetOf (superset: Set<string>, subset: Set<string>): boolean {
  for (const item of subset) {
    if (!superset.has(item)) return false
  }
  return true
}

function isSamePackage<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  depPathA: DepPath,
  depPathB: DepPath
): boolean {
  const nodeA = depGraph[depPathA]
  const nodeB = depGraph[depPathB]
  return nodeA != null && nodeB != null && nodeA.pkgIdWithPatchHash === nodeB.pkgIdWithPatchHash
}
