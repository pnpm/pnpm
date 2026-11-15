import { createPeerDepGraphHash, type PeerId } from '@pnpm/deps.path'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'

import { type AddDepPathContext, addDepPathToGraph, type NodeWithResolvedPeers, peerIdOfResolvedPackage } from './addNodeDepPath.js'
import { getCyclicPeerPredicate, getPeerCacheOwner, propagateCyclicPeerContexts } from './cyclicPeerContexts.js'
import { linkPathToPeerVersion } from './linkPathToPeerVersion.js'
import type { NodeId } from './nextNodeId.js'
import type { PartialResolvedPackage, ResolveNodePeersContext } from './peersResolutionTypes.js'

export interface PeerNodeRecord<Pkg extends PartialResolvedPackage> {
  resolvedNode: NodeWithResolvedPeers<Pkg>
  parentNodeIds: NodeId[]
  parentDepPathsChain: PkgIdWithPatchHash[]
}

export type PeerContextGraph<Pkg extends PartialResolvedPackage> = Pick<ResolveNodePeersContext<Pkg>,
| 'peerNodes' | 'peersCacheOwnerByNodeId' | 'dependenciesTree' | 'pathsByNodeId'
>

type FinalizeContext<Pkg extends PartialResolvedPackage> = PeerContextGraph<Pkg> &
Omit<AddDepPathContext<Pkg>, 'parentNodeIds' | 'parentDepPathsChain'> & {
  dedupePeers?: boolean
  peersSuffixMaxLength: number
}

export function finalizePeerContexts<Pkg extends PartialResolvedPackage> (ctx: FinalizeContext<Pkg>): void {
  if (ctx.peerNodes.size === 0) return
  const isCyclic = getCyclicPeerPredicate(ctx)
  if (isCyclic != null) {
    if (!ctx.dedupePeers) propagateCyclicPeerContexts(ctx, isCyclic)
    rebuildPeerNodes(ctx, calculateFinalPaths(ctx, isCyclic))
  }
  ctx.peerNodes.clear()
}

function rebuildPeerNodes<Pkg extends PartialResolvedPackage> (ctx: FinalizeContext<Pkg>, finalPaths: Map<NodeId, DepPath>): void {
  for (const nodeId of ctx.peerNodes.keys()) {
    const previous = ctx.pathsByNodeId.get(nodeId)
    if (previous != null) delete ctx.depGraph[previous]
  }
  ctx.depPathsByPkgId?.clear()
  for (const record of ctx.peerNodes.values()) {
    addDepPathToGraph({ ...ctx, ...record }, record.resolvedNode, finalPaths.get(record.resolvedNode.nodeId)!)
  }
  updateOccurrencePaths(ctx, finalPaths)
  for (const node of Object.values(ctx.depGraph)) {
    const paths = ctx.depPathsByPkgId?.get(node.pkgIdWithPatchHash) ?? new Set<DepPath>()
    paths.add(node.depPath)
    ctx.depPathsByPkgId?.set(node.pkgIdWithPatchHash, paths)
  }
}

function updateOccurrencePaths<Pkg extends PartialResolvedPackage> (ctx: FinalizeContext<Pkg>, finalPaths: Map<NodeId, DepPath>): void {
  for (const nodeId of ctx.pathsByNodeId.keys()) {
    const finalPath = finalPaths.get(getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, nodeId))
    if (finalPath != null) {
      ctx.pathsByNodeId.set(nodeId, finalPath)
      const node = ctx.dependenciesTree.get(nodeId)
      if (node != null) ctx.depGraph[finalPath].depth = Math.min(ctx.depGraph[finalPath].depth, node.depth)
    }
  }
}

function calculateFinalPaths<Pkg extends PartialResolvedPackage> (
  ctx: FinalizeContext<Pkg>,
  isCyclic: (nodeId: NodeId, alias: string, peerId: NodeId) => boolean
): Map<NodeId, DepPath> {
  const finalPaths = new Map<NodeId, DepPath>()
  function peerIdOf (nodeId: NodeId, alias: string, peer: NodeId): PeerId {
    peer = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)
    if (typeof peer === 'string' && peer.startsWith('link:')) {
      return { name: alias, version: linkPathToPeerVersion(peer.slice(5)) }
    }
    const pkg = ctx.dependenciesTree.get(peer)?.resolvedPackage
    if (pkg != null && (ctx.dedupePeers || isCyclic(nodeId, alias, peer))) {
      return peerIdOfResolvedPackage(pkg)
    }
    return finalPaths.get(peer) ?? ctx.pathsByNodeId.get(peer)!
  }
  for (const nodeId of getPeerFinalizationOrder(ctx, isCyclic)) {
    const { resolvedNode } = ctx.peerNodes.get(nodeId)!
    const peerIds = [...resolvedNode.allResolvedPeers].map(([alias, peer]) => peerIdOf(nodeId, alias, peer))
    const depPath = `${resolvedNode.resolvedPackage.pkgIdWithPatchHash}${createPeerDepGraphHash(peerIds, ctx.peersSuffixMaxLength)}` as DepPath
    finalPaths.set(nodeId, depPath)
  }
  return finalPaths
}

function* getPeerFinalizationOrder<Pkg extends PartialResolvedPackage> (
  ctx: FinalizeContext<Pkg>,
  isCyclic: (nodeId: NodeId, alias: string, peerId: NodeId) => boolean
): Iterable<NodeId> {
  const visited = new Set<NodeId>()
  const pending: Array<[NodeId, boolean]> = [...ctx.peerNodes.keys()].sort().reverse().map((nodeId) => [nodeId, false])
  while (pending.length > 0) {
    const [nodeId, expanded] = pending.pop()!
    if (expanded) {
      yield nodeId
      continue
    }
    if (visited.has(nodeId)) continue
    visited.add(nodeId)
    pending.push([nodeId, true])
    if (ctx.dedupePeers) continue
    for (const owner of getFullPeerPathTargets(ctx, nodeId, isCyclic).reverse()) pending.push([owner, false])
  }
}

function getFullPeerPathTargets<Pkg extends PartialResolvedPackage> (
  ctx: FinalizeContext<Pkg>,
  nodeId: NodeId,
  isCyclic: (nodeId: NodeId, alias: string, peerId: NodeId) => boolean
): NodeId[] {
  const { resolvedNode } = ctx.peerNodes.get(nodeId)!
  return [...resolvedNode.allResolvedPeers].flatMap(([alias, peer]) => {
    const owner = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)
    return ctx.peerNodes.has(owner) && !isCyclic(nodeId, alias, owner) ? [owner] : []
  })
}
