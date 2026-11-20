import { expect, jest, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import { propagateCyclicPeerContexts } from '../lib/cyclicPeerContexts.js'
import type { PeerContextGraph } from '../lib/finalizePeerContexts.js'
import type { NodeId } from '../lib/nextNodeId.js'
import type { PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

test('equivalent cycle occurrences share the provider-context comparison', () => {
  const small = comparisonLookups(40)
  const large = comparisonLookups(80)
  expect(large).toBeLessThan(small * 3)
})

test('propagates distinct external contexts without mutating shared peer maps', () => {
  const ctx: PeerContextGraph<PartialResolvedPackage> = {
    peerNodes: new Map(), dependenciesTree: new Map(), pathsByNodeId: new Map(), peersCacheOwnerByNodeId: new Map(),
  }
  addOccurrence(ctx, 'external', 'external', new Map())
  addOccurrence(ctx, 'other', 'other', new Map())
  const first = new Map([['target', 'target0' as NodeId]])
  const second = new Map([['target', 'target1' as NodeId]])
  addOccurrence(ctx, 'source', 'source0', first)
  addOccurrence(ctx, 'source', 'source1', second)
  addOccurrence(ctx, 'target', 'target0', new Map([['source', 'source0' as NodeId], ['external', 'external' as NodeId]]))
  addOccurrence(ctx, 'target', 'target1', new Map([['source', 'source1' as NodeId], ['external', 'other' as NodeId]]))
  propagateCyclicPeerContexts(ctx, (_nodeId, alias) => alias !== 'external')
  expect(ctx.peerNodes.get('source0' as NodeId)!.resolvedNode.allResolvedPeers.get('external')).toBe('external')
  expect(ctx.peerNodes.get('source1' as NodeId)!.resolvedNode.allResolvedPeers.get('external')).toBe('other')
  expect(first.has('external')).toBe(false)
  expect(second.has('external')).toBe(false)
})

function comparisonLookups (count: number): number {
  const ctx: PeerContextGraph<PartialResolvedPackage> = {
    peerNodes: new Map(),
    dependenciesTree: new Map(),
    pathsByNodeId: new Map(),
    peersCacheOwnerByNodeId: new Map(),
  }
  addOccurrence(ctx, 'external', 'external', new Map())
  for (let index = 0; index < count; index++) {
    addOccurrence(ctx, 'source', `source${index}`, new Map([['target', `target${index}` as NodeId]]))
    addOccurrence(ctx, 'target', `target${index}`, new Map([['source', `source${index}` as NodeId], ['external', 'external' as NodeId]]))
  }
  const lookups = jest.spyOn(ctx.peerNodes, 'get')
  propagateCyclicPeerContexts(ctx, (_nodeId, alias) => alias !== 'external')
  expect(ctx.peerNodes.get('source0' as NodeId)!.resolvedNode.allResolvedPeers.has('external')).toBe(false)
  return lookups.mock.calls.length
}

function addOccurrence (
  ctx: PeerContextGraph<PartialResolvedPackage>,
  name: string,
  id: string,
  peers: Map<string, NodeId>
): void {
  const nodeId = id as NodeId
  const resolvedPackage: PartialResolvedPackage = {
    name,
    version: '1.0.0',
    id: `${name}@1.0.0` as PkgResolutionId,
    pkgIdWithPatchHash: `${name}@1.0.0` as PkgIdWithPatchHash,
    peers: { declared: {}, taken: {} },
  }
  const node = { resolvedPackage, depth: 0, children: {}, installable: true }
  ctx.dependenciesTree.set(nodeId, node)
  ctx.pathsByNodeId.set(nodeId, `${name}@1.0.0` as DepPath)
  ctx.peerNodes.set(nodeId, {
    parentNodeIds: [],
    parentDepPathsChain: [],
    resolvedNode: {
      nodeId, node, resolvedPackage, currentAlias: name, children: {}, isPure: false,
      allResolvedPeers: peers, ownResolvedPeers: new Map(), missingPeersOfChildren: new Map(),
    },
  })
}
