import { expect, test } from '@jest/globals'
import type { DepPath, PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'
import pDefer from 'p-defer'

import { finalizePeerContexts } from '../lib/finalizePeerContexts.js'
import type { NodeId } from '../lib/nextNodeId.js'
import type { PartialResolvedPackage } from '../lib/peersResolutionTypes.js'

test('finalizes a long peer chain ending in a cycle without using the call stack', () => {
  const ctx: Parameters<typeof finalizePeerContexts<PartialResolvedPackage>>[0] = {
    peerNodes: new Map(),
    dependenciesTree: new Map(),
    peersCacheOwnerByNodeId: new Map(),
    pathsByNodeId: new Map(),
    pathsByNodeIdPromises: new Map(),
    depGraph: {},
    depPathsByPkgId: new Map(),
    virtualStoreDir: '/store',
    virtualStoreDirMaxLength: 120,
    peersSuffixMaxLength: 1,
  }
  const nodeCount = 10000
  for (let index = 0; index < nodeCount; index++) {
    const nodeId = index as NodeId
    const target = Math.min(index + 1, nodeCount - 1) as NodeId
    const pkg: PartialResolvedPackage = {
      id: `provider${index}@1.0.0` as PkgResolutionId,
      pkgIdWithPatchHash: `provider${index}@1.0.0` as PkgIdWithPatchHash,
      name: `provider${index}`,
      version: '1.0.0',
      peers: { declared: {}, taken: {} },
    }
    const node = { resolvedPackage: pkg, depth: 0, children: {}, installable: true }
    ctx.dependenciesTree.set(nodeId, node)
    ctx.pathsByNodeId.set(nodeId, pkg.pkgIdWithPatchHash as unknown as DepPath)
    ctx.pathsByNodeIdPromises.set(nodeId, pDefer<DepPath>())
    ctx.peerNodes.set(nodeId, {
      parentNodeIds: [],
      parentDepPathsChain: [],
      resolvedNode: {
        nodeId,
        currentAlias: pkg.name,
        node,
        resolvedPackage: pkg,
        children: {},
        isPure: false,
        allResolvedPeers: new Map([[`provider${target}`, target]]),
        ownResolvedPeers: new Map([[`provider${target}`, target]]),
        missingPeersOfChildren: new Map(),
      },
    })
  }
  finalizePeerContexts(ctx)
  expect(Object.keys(ctx.depGraph)).toHaveLength(nodeCount)
  expect(ctx.pathsByNodeId.get(0 as NodeId)).toMatch(/^provider0@1\.0\.0\([a-f0-9]{32}\)$/)
  const first = ctx.depGraph[ctx.pathsByNodeId.get(0 as NodeId)!]
  expect(first.childrenNodeIds.provider1).toBe(1)
  const last = ctx.depGraph[ctx.pathsByNodeId.get((nodeCount - 1) as NodeId)!]
  expect(last.childrenNodeIds[`provider${nodeCount - 1}`]).toBe(nodeCount - 1)
})
