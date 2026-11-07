import type { DepPath } from '@pnpm/types'
import { analyzeGraph, type Graph } from 'graph-cycles'
import type { DeferredPromise } from 'p-defer'

import type { NodeId } from './nextNodeId.js'
import type { PartialResolvedPackage } from './peersResolutionTypes.js'
import type { DependenciesTree } from './resolveDependencies.js'

interface DepPathAwaitEdges {
  awaitedPeerNodeIdsByNodeId: Map<NodeId, Set<NodeId>>
  peersCacheOwnerByNodeId: Map<NodeId, NodeId>
  cycleBrokenNodeIds: Set<NodeId>
  pathsByNodeId: Map<NodeId, DepPath>
}

// Cycle analysis inside resolvePeersOfChildren only sees the children of a
// single call, but a dep path calculation may await a node resolved in a
// different call or at a different tree level (a hoisted peer provider and
// a consumer that peer-depend on each other, https://github.com/pnpm/pnpm/issues/12921).
// Such an await cycle never settles on its own, so before the finishing
// promises are awaited, walk the recorded await edges and resolve every
// dep path promise on a cycle to the peer's `name@version` — the same
// collapse in-call cycle detection applies (see calculateDepPath).
// A node that hit the peers cache awaits its dep path from the node that
// created the cache entry, so it borrows that owner's await edges.
export function breakDepPathAwaitCycles<Pkg extends PartialResolvedPackage> (
  opts: DepPathAwaitEdges & {
    pathsByNodeIdPromises: Map<NodeId, DeferredPromise<DepPath>>
    dependenciesTree: DependenciesTree<Pkg>
  }
): void {
  const { graphEntries, nodeIdsByKey } = collectLiveAwaitGraph(opts)
  if (graphEntries.length === 0) return
  const { cycles } = analyzeGraph(graphEntries as unknown as Graph) as unknown as { cycles: string[][] }
  for (const key of new Set(cycles.flat())) {
    const nodeId = nodeIdsByKey.get(key)!
    const { name, version } = opts.dependenciesTree.get(nodeId)!.resolvedPackage
    opts.cycleBrokenNodeIds.add(nodeId)
    opts.pathsByNodeIdPromises.get(nodeId)?.resolve(`${name}@${version}` as DepPath)
  }
}

function collectLiveAwaitGraph (
  opts: DepPathAwaitEdges
): { graphEntries: Array<[string, string[]]>, nodeIdsByKey: Map<string, NodeId> } {
  const isSettled = (nodeId: NodeId): boolean =>
    opts.pathsByNodeId.has(nodeId) || opts.cycleBrokenNodeIds.has(nodeId)
  const nodeKeys = createNodeKeys()
  const graphEntries: Array<[string, string[]]> = []
  const awaitingNodeIds = new Set([
    ...opts.awaitedPeerNodeIdsByNodeId.keys(),
    ...opts.peersCacheOwnerByNodeId.keys(),
  ])
  for (const nodeId of awaitingNodeIds) {
    if (isSettled(nodeId)) continue
    const awaitedNodeIds = getAwaitedNodeIds(opts, nodeId)
    if (awaitedNodeIds == null) continue
    const liveTargets = [...awaitedNodeIds]
      .filter((awaitedNodeId) => !isSettled(awaitedNodeId))
      .map((awaitedNodeId) => nodeKeys.keyOf(awaitedNodeId))
    if (liveTargets.length > 0) {
      graphEntries.push([nodeKeys.keyOf(nodeId), liveTargets])
    }
  }
  return { graphEntries, nodeIdsByKey: nodeKeys.nodeIdsByKey }
}

function getAwaitedNodeIds (opts: DepPathAwaitEdges, nodeId: NodeId): Set<NodeId> | undefined {
  const cacheOwnerNodeId = opts.peersCacheOwnerByNodeId.get(nodeId)
  return opts.awaitedPeerNodeIdsByNodeId.get(nodeId) ??
    (cacheOwnerNodeId == null ? undefined : opts.awaitedPeerNodeIdsByNodeId.get(cacheOwnerNodeId))
}

function createNodeKeys (): { keyOf: (nodeId: NodeId) => string, nodeIdsByKey: Map<string, NodeId> } {
  const keysByNodeId = new Map<NodeId, string>()
  const nodeIdsByKey = new Map<string, NodeId>()
  function keyOf (nodeId: NodeId): string {
    let key = keysByNodeId.get(nodeId)
    if (key == null) {
      key = String(keysByNodeId.size)
      keysByNodeId.set(nodeId, key)
      nodeIdsByKey.set(key, nodeId)
    }
    return key
  }
  return { keyOf, nodeIdsByKey }
}
