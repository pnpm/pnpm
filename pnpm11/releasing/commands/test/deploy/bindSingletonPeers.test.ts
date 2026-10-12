import { expect, test } from '@jest/globals'
import type { PackageSnapshot, PackageSnapshots, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

import { bindSingletonPeers, type LinkedWorkspaceProject } from '../../src/deploy/deployPackageGraph.js'

const LIB = 'lib@file:lib' as DepPath

function snapshot (dependencies?: ResolvedDependencies): PackageSnapshot {
  return { resolution: { integrity: 'sha512-AA==' }, dependencies }
}

/**
 * Binds the `peer` peer of `lib`, which `mid` and `mid-2` depend on. `other`
 * brings `peer@2.0.0` into the graph when `withOtherCopy` is set.
 */
function bindPeerOfLib (midPeer: string, mid2Peer: string, withOtherCopy: boolean): PackageSnapshots {
  const importer: ProjectSnapshot = {
    specifiers: {},
    dependencies: { mid: '1.0.0', 'mid-2': '1.0.0', ...(withOtherCopy ? { other: '1.0.0' } : {}) },
  }
  const packages: PackageSnapshots = {
    [LIB]: snapshot(),
    ['mid@1.0.0' as DepPath]: snapshot({ lib: 'file:lib', peer: midPeer }),
    ['mid-2@1.0.0' as DepPath]: snapshot({ lib: 'file:lib', peer: mid2Peer }),
    ...(withOtherCopy ? { ['other@1.0.0' as DepPath]: snapshot({ peer: '2.0.0' }) } : {}),
  }
  bindSingletonPeers(importer, packages, new Map([[LIB, {
    manifest: { name: 'lib', version: '1.0.0', peerDependencies: { peer: '*' } },
    dedupedPeerResolutions: undefined,
  }]]))
  return packages
}

// The graph-wide resolutions hold a second version, so only the parents can
// bind the peer.
test('bindSingletonPeers counts two spellings of one package once', () => {
  expect(bindPeerOfLib('1.0.0', 'peer@1.0.0', true)[LIB].dependencies?.peer).toBeDefined()
})

// A `<root>/` link points into the package that declares it, so it names a
// different package than the other parent's copy. That copy is the graph's
// only other resolution, since a link is not one.
test('bindSingletonPeers refuses a link into a providing package next to another provider', () => {
  expect(() => bindPeerOfLib('link:<root>/vendor/peer', '1.0.0', false)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_DEPLOY_AMBIGUOUS_PEER',
  }))
})

// sibling consumes lib as a peer, so it is not lib's parent, and its own copy
// of the peer does not compete with the deployed project's.
test('bindSingletonPeers does not treat a package consuming the linked package as a peer as its parent', () => {
  const importer: ProjectSnapshot = {
    specifiers: {},
    dependencies: { lib: 'file:lib', sibling: '1.0.0', peer: '1.0.0' },
  }
  const packages: PackageSnapshots = {
    [LIB]: snapshot(),
    ['sibling@1.0.0' as DepPath]: { ...snapshot({ lib: 'file:lib', peer: '2.0.0' }), peerDependencies: { lib: '*' } },
    ['peer@1.0.0' as DepPath]: snapshot(),
    ['peer@2.0.0' as DepPath]: snapshot(),
  }
  bindSingletonPeers(importer, packages, new Map([[LIB, {
    manifest: { name: 'lib', version: '1.0.0', peerDependencies: { peer: '*' } },
    dedupedPeerResolutions: undefined,
  }]]))
  expect(packages[LIB].dependencies?.peer).toBe('1.0.0')
})

// consumer lists lib as both a dependency and a peer, so the dependency wins
// and consumer is lib's parent, the only one.
test('bindSingletonPeers binds to a parent that also declares the linked package as a peer', () => {
  const CONSUMER = 'consumer@file:consumer' as DepPath
  const importer: ProjectSnapshot = {
    specifiers: {},
    dependencies: { consumer: 'file:consumer', peer: '1.0.0' },
  }
  const packages: PackageSnapshots = {
    [LIB]: snapshot(),
    [CONSUMER]: snapshot({ lib: 'file:lib', peer: '1.0.1' }),
    ['peer@1.0.0' as DepPath]: snapshot(),
    ['peer@1.0.1' as DepPath]: snapshot(),
  }
  bindSingletonPeers(importer, packages, new Map<DepPath, LinkedWorkspaceProject>([
    [LIB, {
      manifest: { name: 'lib', version: '1.0.0', peerDependencies: { peer: '*' } },
      dedupedPeerResolutions: undefined,
    }],
    [CONSUMER, {
      manifest: { name: 'consumer', version: '1.0.0', dependencies: { lib: 'workspace:*' }, peerDependencies: { lib: 'workspace:*' } },
      dedupedPeerResolutions: undefined,
    }],
  ]))
  expect(packages[LIB].dependencies?.peer).toBe('1.0.1')
})
