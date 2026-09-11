import { expect, test } from '@jest/globals'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'

import { collectMissingRequiredPeers, type PeerDependencies } from '../lib/resolveDependencies.js'

test('collects required peers through a dense dependency cycle', () => {
  const ids = Array.from({ length: 20 }, (_, index) => `pkg-${index}`)
  const ctx = createGraph(ids.map((id) => ({
    id,
    children: ids.filter((other) => other !== id),
    peers: { peer: { version: '^1.0.0' } },
  })))
  expect(collectMissingRequiredPeers(ctx, [root(ids[0])])).toStrictEqual({
    peer: { range: '>=1.0.0 <2.0.0-0', optional: false },
  })
})

test('collects peers through a graph deeper than the call stack', () => {
  const ctx = createGraph(Array.from({ length: 20_000 }, (_, index) => ({
    id: `pkg-${index}`,
    children: index === 19_999 ? [] : [`pkg-${index + 1}`],
    peers: index === 19_999 ? { peer: { version: '1.0.0' } } : undefined,
  })))
  expect(collectMissingRequiredPeers(ctx, [root('pkg-0')])).toStrictEqual({
    peer: { range: '1.0.0', optional: false },
  })
})

test('a provider only satisfies peers in the subtree where it is available', () => {
  const ctx = createGraph([
    { id: 'provided', children: ['child', 'peer'] },
    { id: 'missing', children: ['child'] },
    { id: 'child', peers: { peer: { version: '1.0.0' }, optional: { version: '*', optional: true } } },
    { id: 'peer' },
  ])
  expect(collectMissingRequiredPeers(ctx, [root('provided')])).toStrictEqual({})
  expect(collectMissingRequiredPeers(ctx, [root('missing'), root('provided')])).toStrictEqual({
    peer: { range: '1.0.0', optional: false },
  })
  expect(collectMissingRequiredPeers(ctx, [root('missing'), root('peer')])).toStrictEqual({})
})

function root (id: string): { alias: string, pkgId: PkgResolutionId } {
  return { alias: id, pkgId: id as PkgResolutionId }
}

function createGraph (nodes: Array<{ id: string, children?: string[], peers?: PeerDependencies }>): Parameters<typeof collectMissingRequiredPeers>[0] {
  return {
    autoInstallPeersFromHighestMatch: false,
    childrenByParentId: Object.fromEntries(nodes.map(({ id, children }) => [
      id, (children ?? []).map((child) => ({ alias: child, id: child as PkgResolutionId })),
    ])),
    resolvedPkgsById: Object.fromEntries(nodes.map(({ id, peers }) => [id, { peerDependencies: peers ?? {} }])),
  }
}
