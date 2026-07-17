import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

import type { NodeId } from './nextNodeId.js'
import type { DependenciesTree } from './resolveDependencies.js'
import type {
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
} from './resolvePeers.js'

interface FreshContext {
  children: Set<DepPath>
  peerContext: Map<string, DepPath>
  resolvedPeerNames: Set<string>
}

export function getDedupeOnlyLockedPeerUnions<T extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<T>,
  resolved: {
    dependenciesGraph: GenericDependenciesGraphWithResolvedChildren<T>
    pathsByNodeId: Map<NodeId, DepPath>
  },
  candidatesByNodeId: Map<NodeId, Map<string, DepPath>>
): {
  peerNamesByNodeId: Map<NodeId, Set<string>>
  peersCacheBypassNodeIds: Set<NodeId>
} {
  if (candidatesByNodeId.size === 0) {
    return {
      peerNamesByNodeId: new Map(),
      peersCacheBypassNodeIds: new Set(),
    }
  }

  const candidatePkgIds = new Set([...candidatesByNodeId.keys()]
    .map((nodeId) => (dependenciesTree.get(nodeId)!.resolvedPackage as T).pkgIdWithPatchHash))
  const freshPeerContextsByPkgId = new Map<PkgIdWithPatchHash, Map<string, FreshContext[]>>()
  const freshPeerContextByNodeId = new Map<NodeId, FreshContext>()
  for (const [nodeId, depPath] of resolved.pathsByNodeId) {
    const node = dependenciesTree.get(nodeId)
    const resolvedNode = resolved.dependenciesGraph[depPath]
    if (node == null || node.depth === -1 || resolvedNode == null) continue
    const pkg = node.resolvedPackage as T
    const pkgId = pkg.pkgIdWithPatchHash
    if (!candidatePkgIds.has(pkgId)) continue
    const peerContext = new Map(Object.keys(resolvedNode.peerDependencies ?? {})
      .filter((peerName) => pkg.peerDependencies[peerName]?.optional === true)
      .flatMap((peerName) => {
        const peerDepPath = resolvedNode.children[peerName]
        return resolvedNode.resolvedPeerNames.has(peerName) && peerDepPath != null
          ? [[peerName, peerDepPath] as const]
          : []
      }))
    const signature = [...peerContext].sort(([peerName1], [peerName2]) => peerName1.localeCompare(peerName2))
      .map(([peerName, peerDepPath]) => `${peerName}=${peerDepPath}`)
      .join('\0')
    const freshContext = {
      children: new Set(Object.values(resolvedNode.children)),
      peerContext,
      resolvedPeerNames: resolvedNode.resolvedPeerNames,
    }
    freshPeerContextByNodeId.set(nodeId, freshContext)
    let contexts = freshPeerContextsByPkgId.get(pkgId)
    if (contexts == null) {
      contexts = new Map()
      freshPeerContextsByPkgId.set(pkgId, contexts)
    }
    const matchingContexts = contexts.get(signature)
    if (matchingContexts == null) {
      contexts.set(signature, [freshContext])
    } else {
      matchingContexts.push(freshContext)
    }
  }

  const allowedPeerNamesByNodeId = new Map<NodeId, Set<string>>()
  for (const [nodeId, candidate] of candidatesByNodeId) {
    const node = dependenciesTree.get(nodeId)!
    const pkg = node.resolvedPackage as T
    const freshContext = freshPeerContextByNodeId.get(nodeId)
    if (
      freshContext == null ||
      freshContext.peerContext.size === 0 ||
      !isPeerContextSubset(freshContext.peerContext, candidate)
    ) continue
    const candidateChildren = new Set([...freshContext.children, ...candidate.values()])
    const candidatePeerNames = new Set([...freshContext.resolvedPeerNames, ...candidate.keys()])
    const freshContexts = [...(freshPeerContextsByPkgId.get(pkg.pkgIdWithPatchHash)?.values() ?? [])]
    // Admit only a genuinely new union: this occurrence must contribute a
    // non-empty exact subset, at least one other fresh context must contribute,
    // and the combined provider map must equal the locked context.
    if (freshContexts.some((contexts) => contexts.some(({ peerContext }) => mapsEqual(peerContext, candidate)))) continue
    const contributingContexts = freshContexts.flatMap((contexts) => {
      const context = contexts.find((context) =>
        context.peerContext.size > 0 &&
        isPeerContextSubset(context.peerContext, candidate) &&
        [...context.children].every((depPath) => candidateChildren.has(depPath)) &&
        [...context.resolvedPeerNames].every((peerName) => candidatePeerNames.has(peerName))
      )
      return context == null ? [] : [context.peerContext]
    })
    if (contributingContexts.length < 2) continue
    const supportedContext = new Map(contributingContexts.flatMap((context) => [...context]))
    if (mapsEqual(supportedContext, candidate)) {
      allowedPeerNamesByNodeId.set(nodeId, new Set(candidate.keys()))
    }
  }
  const peersCacheBypassNodeIds = getPeersCacheBypassNodeIds(dependenciesTree, allowedPeerNamesByNodeId.keys())
  return {
    peerNamesByNodeId: allowedPeerNamesByNodeId,
    peersCacheBypassNodeIds,
  }
}

export function getLockedOptionalPeerUnionCandidates<T extends PartialResolvedPackage> (
  dependenciesTree: DependenciesTree<T>
): Map<NodeId, Map<string, DepPath>> {
  const occurrencesByPkgId = new Map<PkgIdWithPatchHash, number>()
  for (const node of dependenciesTree.values()) {
    if (node.depth === -1) continue
    const pkgId = (node.resolvedPackage as T).pkgIdWithPatchHash
    occurrencesByPkgId.set(pkgId, (occurrencesByPkgId.get(pkgId) ?? 0) + 1)
  }
  const candidatesByNodeId = new Map<NodeId, Map<string, DepPath>>()
  for (const [nodeId, node] of dependenciesTree) {
    if (node.depth === -1 || node.lockedPeerContext == null) continue
    const pkg = node.resolvedPackage as T
    if ((occurrencesByPkgId.get(pkg.pkgIdWithPatchHash) ?? 0) < 2) continue
    const candidate = new Map(Object.entries(node.lockedPeerContext)
      .filter(([peerName]) => pkg.peerDependencies[peerName]?.optional === true))
    if (candidate.size >= 2) {
      candidatesByNodeId.set(nodeId, candidate)
    }
  }
  return candidatesByNodeId
}

export function getPeersCacheBypassNodeIds (
  dependenciesTree: DependenciesTree<unknown>,
  nodeIds: Iterable<NodeId>
): Set<NodeId> {
  const pendingNodeIds = [...nodeIds]
  const peersCacheBypassNodeIds = new Set<NodeId>()
  while (pendingNodeIds.length > 0) {
    const nodeId = pendingNodeIds.pop()!
    if (peersCacheBypassNodeIds.has(nodeId)) continue
    peersCacheBypassNodeIds.add(nodeId)
    const parentNodeId = dependenciesTree.get(nodeId)?.parentNodeId
    if (parentNodeId != null) pendingNodeIds.push(parentNodeId)
  }
  return peersCacheBypassNodeIds
}

function mapsEqual<K, V> (map1: Map<K, V>, map2: Map<K, V>): boolean {
  return map1.size === map2.size && [...map1].every(([key, value]) => map2.get(key) === value)
}

function isPeerContextSubset (
  context: Map<string, DepPath>,
  candidate: Map<string, DepPath>
): boolean {
  return [...context].every(([peerName, peerDepPath]) => candidate.get(peerName) === peerDepPath)
}
