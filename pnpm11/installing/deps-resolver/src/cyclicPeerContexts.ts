import { computeStronglyConnectedComponents } from '@pnpm/deps.graph-sequencer'

import type { PeerContextGraph } from './finalizePeerContexts.js'
import type { NodeId } from './nextNodeId.js'
import type { PartialResolvedPackage } from './peersResolutionTypes.js'

export function getPeerCacheOwner (owners: Map<NodeId, NodeId>, nodeId: NodeId): NodeId {
  let owner = owners.get(nodeId)
  while (owner != null && owner !== nodeId) {
    nodeId = owner
    owner = owners.get(nodeId)
  }
  return nodeId
}

export function getCyclicPeerPredicate<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>
): ((nodeId: NodeId, alias: string, peerId: NodeId) => boolean) | undefined {
  const nodeEdges = new Map<NodeId, Set<NodeId>>()
  const nameEdges = new Map<string, Set<string>>()
  for (const [nodeId, { resolvedNode }] of ctx.peerNodes) {
    const peers = resolvedNode.allResolvedPeers
    nodeEdges.set(nodeId, new Set([...peers.values()].map((peer) => getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer))))
    const name = resolvedNode.resolvedPackage.name
    const names = nameEdges.get(name) ?? new Set<string>()
    for (const alias of peers.keys()) names.add(alias)
    nameEdges.set(name, names)
  }
  const nodeComponents = getComponents(nodeEdges)
  const cyclicNames = getCyclicKeys(nameEdges, getComponents(nameEdges))
  if (cyclicNames.size === 0 && getCyclicKeys(nodeEdges, nodeComponents).size === 0) return undefined
  return (nodeId, alias, peerId) => cyclicNames.has(alias) ||
    (nodeComponents.has(nodeId) && nodeComponents.get(nodeId) === nodeComponents.get(peerId))
}

function getCyclicKeys<Key> (edges: Map<Key, Set<Key>>, components: Map<Key, number>): Set<Key> {
  const cyclicNames = new Set<Key>()
  for (const [name, peers] of edges) {
    if ([...peers].some((peer) => components.get(name) === components.get(peer))) cyclicNames.add(name)
  }
  return cyclicNames
}

function getComponents<Key> (edges: Map<Key, Set<Key>>): Map<Key, number> {
  const keys = [...new Set([...edges.keys(), ...[...edges.values()].flatMap((targets) => [...targets])])]
  const indices = new Map(keys.map((key, index) => [key, index]))
  const adjacency = keys.map((key) => [...edges.get(key) ?? []].map((target) => indices.get(target)!))
  const { componentOf } = computeStronglyConnectedComponents(adjacency, keys.map(() => false))
  return new Map(keys.map((key, index) => [key, componentOf[index]]))
}

interface PropagationState {
  occurrences: Map<string, Set<NodeId>>
  modifiedNodes: Set<NodeId>
  varyingPeers: Map<Set<NodeId>, Map<string, boolean>>
}

export function propagateCyclicPeerContexts<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  isCyclic: (nodeId: NodeId, alias: string, peerId: NodeId) => boolean
): void {
  const state: PropagationState = {
    occurrences: getOccurrencesByPackage(ctx),
    modifiedNodes: new Set(),
    varyingPeers: new Map(),
  }
  const candidates = [...ctx.peerNodes.keys()].sort()
  let changed: boolean
  do {
    changed = false
    for (const nodeId of candidates) {
      const { resolvedNode } = ctx.peerNodes.get(nodeId)!
      const targets = [...resolvedNode.allResolvedPeers]
        .map(([alias, peer]) => [alias, getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)] as const)
        .filter(([alias, peer]) => isCyclic(nodeId, alias, peer))
        .sort(([left], [right]) => left.localeCompare(right))
      for (const [, target] of targets) {
        changed = propagateTarget(ctx, { nodeId, target, state }) || changed
      }
    }
  } while (changed)
}

function getOccurrencesByPackage<Pkg extends PartialResolvedPackage> (ctx: PeerContextGraph<Pkg>): Map<string, Set<NodeId>> {
  const occurrences = new Map<string, Set<NodeId>>()
  for (const nodeId of ctx.pathsByNodeId.keys()) {
    const owner = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, nodeId)
    const pkg = ctx.dependenciesTree.get(owner)?.resolvedPackage
    if (pkg == null || !('pkgIdWithPatchHash' in pkg)) continue
    const nodes = occurrences.get(pkg.pkgIdWithPatchHash) ?? new Set<NodeId>()
    nodes.add(owner)
    occurrences.set(pkg.pkgIdWithPatchHash, nodes)
  }
  return occurrences
}

function propagateTarget<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  opts: { nodeId: NodeId, target: NodeId, state: PropagationState }
): boolean {
  const { resolvedNode } = ctx.peerNodes.get(opts.nodeId)!
  const target = ctx.peerNodes.get(opts.target)?.resolvedNode
  if (target == null) return false
  const occurrences = opts.state.occurrences.get(target.resolvedPackage.pkgIdWithPatchHash)!
  let changed = false
  for (const [alias, peer] of target.allResolvedPeers) {
    const owner = getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, peer)
    if (!canPropagatePeer(ctx, { nodeId: opts.nodeId, alias, owner })) continue
    if (!peerVariesAcrossOccurrences(ctx, { owner, alias, occurrences, cache: opts.state.varyingPeers })) continue
    if (!opts.state.modifiedNodes.has(opts.nodeId)) {
      resolvedNode.allResolvedPeers = new Map(resolvedNode.allResolvedPeers)
      opts.state.modifiedNodes.add(opts.nodeId)
    }
    resolvedNode.allResolvedPeers.set(alias, peer)
    opts.state.varyingPeers.clear()
    changed = true
  }
  return changed
}

function peerVariesAcrossOccurrences<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  opts: { owner: NodeId, alias: string, occurrences: Set<NodeId>, cache: PropagationState['varyingPeers'] }
): boolean {
  const aliases = opts.cache.get(opts.occurrences) ?? new Map<string, boolean>()
  const cached = aliases.get(opts.alias)
  if (cached != null) return cached
  const varies = [...opts.occurrences].some((other) => {
    const otherPeer = ctx.peerNodes.get(other)?.resolvedNode.allResolvedPeers.get(opts.alias)
    return otherPeer == null || !isSamePeerOccurrence(ctx, [opts.owner, otherPeer])
  })
  aliases.set(opts.alias, varies)
  opts.cache.set(opts.occurrences, aliases)
  return varies
}

function canPropagatePeer<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  opts: { nodeId: NodeId, alias: string, owner: NodeId }
): boolean {
  const { resolvedNode } = ctx.peerNodes.get(opts.nodeId)!
  if (resolvedNode.allResolvedPeers.has(opts.alias) || Object.hasOwn(resolvedNode.resolvedPackage.peers.taken, opts.alias) || opts.owner === opts.nodeId) return false
  const peerPackage = ctx.dependenciesTree.get(opts.owner)?.resolvedPackage
  return opts.alias !== resolvedNode.resolvedPackage.name ||
    (peerPackage != null && 'pkgIdWithPatchHash' in peerPackage && peerPackage.pkgIdWithPatchHash !== resolvedNode.resolvedPackage.pkgIdWithPatchHash)
}

function isSamePeerOccurrence<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  pair: [NodeId, NodeId]
): boolean {
  const visited = new Map<NodeId, Set<NodeId>>()
  const pending = [pair]
  while (pending.length > 0) {
    const [left, right] = pending.pop()!.map((nodeId) => getPeerCacheOwner(ctx.peersCacheOwnerByNodeId, nodeId))
    if (left === right || visited.get(left)?.has(right)) continue
    const rightIds = visited.get(left) ?? new Set<NodeId>()
    rightIds.add(right)
    visited.set(left, rightIds)
    if (!comparePeerTargets(ctx, [left, right], pending)) return false
  }
  return true
}

function comparePeerTargets<Pkg extends PartialResolvedPackage> (
  ctx: PeerContextGraph<Pkg>,
  [left, right]: [NodeId, NodeId],
  pending: Array<[NodeId, NodeId]>
): boolean {
  const leftPkg = ctx.dependenciesTree.get(left)?.resolvedPackage
  const rightPkg = ctx.dependenciesTree.get(right)?.resolvedPackage
  if (leftPkg == null || rightPkg == null || !('pkgIdWithPatchHash' in leftPkg) || !('pkgIdWithPatchHash' in rightPkg) || leftPkg.pkgIdWithPatchHash !== rightPkg.pkgIdWithPatchHash) return false
  const leftPeers = ctx.peerNodes.get(left)?.resolvedNode.allResolvedPeers
  const rightPeers = ctx.peerNodes.get(right)?.resolvedNode.allResolvedPeers
  if ((leftPeers?.size ?? 0) !== (rightPeers?.size ?? 0)) return false
  for (const [alias, peer] of leftPeers ?? []) {
    const other = rightPeers?.get(alias)
    if (other == null) return false
    pending.push([peer, other])
  }
  return true
}
