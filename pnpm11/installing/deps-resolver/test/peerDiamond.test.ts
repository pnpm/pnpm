import { expect, test } from '@jest/globals'
import type { PkgIdWithPatchHash, PkgResolutionId } from '@pnpm/types'

import type { NodeId } from '../lib/nextNodeId.js'
import { inheritedParentPkgBreaksPeerDiamond } from '../lib/peerDiamond.js'
import type { ParentPkgInfo, ParentRef, PartialResolvedPackage } from '../lib/peersResolutionTypes.js'
import type { PeerDependencies } from '../lib/resolutionTypes.js'
import type { DependenciesTreeNode } from '../lib/resolveDependencies.js'

const INHERITED = '>parser>' as NodeId
const OWN = '>app>parser>' as NodeId
const DESCENDANT = '>app>plugin>' as NodeId

const provider: ParentRef = { version: '1.0.0', occurrence: 0, depth: 0, parentNodeIds: [] }

test.each(['constructor', 'toString'])('diamond detection requires own %s providers and peer declarations', (peerName) => {
  const inheritedProvider: ParentPkgInfo = { version: '2.0.0' }
  const ownPeers = { [peerName]: { version: '*' } }
  const cases = [
    { inherited: {}, current: { [peerName]: provider }, descendant: { parser: { version: '*' }, ...ownPeers }, expected: false },
    { inherited: { [peerName]: inheritedProvider }, current: {}, descendant: { parser: { version: '*' }, ...ownPeers }, expected: false },
    { inherited: { [peerName]: inheritedProvider }, current: { [peerName]: provider }, descendant: { parser: { version: '*' } }, expected: false },
    { inherited: { [peerName]: inheritedProvider }, current: { [peerName]: provider }, descendant: { parser: { version: '*' }, ...ownPeers }, expected: true },
  ]
  for (const entry of cases) {
    const tree = new Map<NodeId, DependenciesTreeNode<PartialResolvedPackage>>([
      [OWN, node('parser', ownPeers)],
      [DESCENDANT, node('plugin', entry.descendant)],
    ])
    expect(inheritedParentPkgBreaksPeerDiamond({
      dependenciesTree: tree,
      parentPkgsOfNode: new Map([[INHERITED, entry.inherited]]),
      allPeerDepNames: new Set([peerName]),
    }, entry.current, {
      inheritedParentPkg: { ...provider, nodeId: INHERITED },
      ownChildParentPkg: { ...provider, nodeId: OWN },
      children: { plugin: DESCENDANT },
    })).toBe(entry.expected)
  }
})

function node (name: string, peers: PeerDependencies): DependenciesTreeNode<PartialResolvedPackage> {
  return {
    children: {},
    depth: 1,
    installable: true,
    resolvedPackage: {
      name,
      version: '1.0.0',
      pkgIdWithPatchHash: `${name}/1.0.0` as PkgIdWithPatchHash,
      id: '' as PkgResolutionId,
      peers: { declared: peers, taken: peers },
    },
  }
}
