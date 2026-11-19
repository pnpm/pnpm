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
  const cyclicPeer = getCyclicPeerPredicate(ctx)
  if (ctx.dedupePeers || cyclicPeer != null) {
    const isCyclic = cyclicPeer ?? (() => false)
    if (cyclicPeer != null) propagateCyclicPeerContexts(ctx, cyclicPeer)
    const fullPaths = calculateFinalPaths(ctx, isCyclic)
    const paths = ctx.dedupePeers
      ? calculateFinalPaths(ctx, isCyclic, findAmbiguousPeers(ctx, fullPaths))
      : fullPaths
    rebuildPeerNodes(ctx, paths)
  }
  ctx.peerNodes.clear()
}

function findAmbiguousPeers<Pkg extends PartialResolvedPackage> (
  ctx: FinalizeContext<Pkg>,
  fullPaths: Map<NodeId, DepPath>
): Set<string> {
  const pathsByPeer = new Map<string, DepPath>()
  const ambiguousPeers = new Set<string>()
  for (const { resolvedNode } of ctx.peerNodes.values()) {
    for (const peer of resolvedNode.allResolvedPeers.values()) {
      const entry = getFlatPeerPath(ctx, fullPaths, peer)
      if (entry == null) continue
      const [key, path] = entry
      const previous = pathsByPeer.get(key)
      if (previous == null) pathsByPeer.set(key, path)
      else if (previous !== path) ambiguousPeers.add(key)
    }
  }
  return ambiguousPeers
}

function getFlatPeerPath<Pkg extends PartialResolvedPackage> (
  ctx: FinalizeContext<Pkg>,
  fullPaths: Map<NodeId, DepPath>,
  peer: NodeId
): [string, DepPath] | undefined {
  const owner = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)
  const pkg = ctx.dependenciesTree.get(owner)?.resolvedPackage
  const path = fullPaths.get(owner) ?? ctx.pathsByNodeId.get(owner)
  if (pkg != null && path != null) return [flatPeerKey(pkg), path]
  return undefined
}

function flatPeerKey (pkg: Parameters<typeof peerIdOfResolvedPackage>[0]): string {
  const { name, version } = peerIdOfResolvedPackage(pkg)
  return `${name}@${version}`
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
  isCyclic: (nodeId: NodeId, alias: string, peerId: NodeId) => boolean,
  ambiguousPeers?: Set<string>
): Map<NodeId, DepPath> {
  const finalPaths = new Map<NodeId, DepPath>()
  function peerIdOf (nodeId: NodeId, alias: string, peer: NodeId): PeerId {
    peer = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)
    if (typeof peer === 'string' && peer.startsWith('link:')) {
      return { name: alias, version: linkPathToPeerVersion(peer.slice(5)) }
    }
    const pkg = ctx.dependenciesTree.get(peer)?.resolvedPackage
    if (pkg != null && (isCyclic(nodeId, alias, peer) || (ambiguousPeers != null && !ambiguousPeers.has(flatPeerKey(pkg))))) {
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
