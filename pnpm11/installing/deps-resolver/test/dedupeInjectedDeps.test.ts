import { expect, test } from '@jest/globals'
import type { DepPath, PkgResolutionId } from '@pnpm/types'

import { dedupeInjectedDeps, type DedupeInjectedDepsOptions } from '../lib/dedupeInjectedDeps.js'
import type { NodeId } from '../lib/nextNodeId.js'
import type { PartialResolvedPackage } from '../lib/resolvePeers.js'

type Opts = DedupeInjectedDepsOptions<PartialResolvedPackage>

// Regression test for https://github.com/pnpm/pnpm/issues/10433.
//
// An injected workspace dependency should dedupe back to `link:` whenever its
// resolved children match the target project's own dependencies. That match
// must tolerate a completely unrelated, ordinary (non-workspace) shared
// dependency resolving to a peer-suffixed variant on one side and a
// peer-free variant on the other -- both are valid resolutions of the same
// package, and the mismatch is incidental to pkg-c itself. This is exactly
// what a real install produces once an existing lockfile pins an optional
// peer (e.g. `debug`'s optional `supports-color`) for one occurrence but not
// the other: the injected occurrence resolves fresh while the target project's
// own occurrence inherits the pin. Without the compatibility tolerance in
// `getDedupeMap`, a strict string-equality check would strand the entry as
// `file:(...)` instead of collapsing it to `link:`.
test('injected dependency dedupes to link: even when an unrelated shared dependency has a peer suffix on only one side', () => {
  const nodeId = 1 as NodeId
  const depPath = '@scope/pkg-c@file:packages/pkg-c(@scope/pkg-a@file:packages/pkg-a)' as DepPath

  const directNodeIdsByAlias = new Map([['@scope/pkg-c', nodeId]])

  const depGraph = {
    [depPath]: {
      id: 'file:packages/pkg-c' as PkgResolutionId,
      pkgIdWithPatchHash: 'file:packages/pkg-c',
      children: {
        // Freshly resolved (no pre-existing lockfile pin): debug has no peer suffix.
        debug: 'debug@4.4.3' as DepPath,
        '@scope/pkg-a': '@scope/pkg-a@file:packages/pkg-a' as DepPath,
      },
    },
    // debug's own two resolutions: peer-free (as seen by the injected occurrence)
    // and pinned-to-its-optional-peer (as seen by the target project's own copy).
    // Both share the same `pkgIdWithPatchHash` (debug@4.4.3) — they are the same
    // package+version, differing only by peer suffix.
    'debug@4.4.3': {
      pkgIdWithPatchHash: 'debug@4.4.3',
      children: {},
      resolvedPeerNames: new Set(),
    },
    'debug@4.4.3(supports-color@8.1.1)': {
      pkgIdWithPatchHash: 'debug@4.4.3',
      children: { 'supports-color': 'supports-color@8.1.1' as DepPath },
      resolvedPeerNames: new Set(['supports-color']),
    },
  } as unknown as Opts['depGraph']

  const dependenciesByProjectId = {
    'packages/consumer': new Map<string, DepPath>([
      ['@scope/pkg-c', depPath],
    ]),
    'packages/pkg-c': new Map<string, DepPath>([
      ['@scope/pkg-a', '@scope/pkg-a@file:packages/pkg-a' as DepPath],
      // Same underlying debug@4.4.3, but pinned to its optional peer
      // (supports-color) by the existing lockfile -- a valid, equally-correct
      // resolution.
      ['debug', 'debug@4.4.3(supports-color@8.1.1)' as DepPath],
    ]),
  }

  const resolvedImporters: Opts['resolvedImporters'] = {
    'packages/consumer': {
      directDependencies: [
        {
          alias: '@scope/pkg-c',
          pkgId: 'file:packages/pkg-c' as PkgResolutionId,
        } as Opts['resolvedImporters'][string]['directDependencies'][number],
      ],
      directNodeIdsByAlias,
      hoistedPeerProviderNodeIds: new Set<NodeId>(),
      linkedDependencies: [],
    },
  }

  dedupeInjectedDeps({
    depGraph,
    dependenciesByProjectId,
    lockfileDir: '/repo',
    pathsByNodeId: new Map([[nodeId, depPath]]),
    projects: [{ id: 'packages/consumer', directNodeIdsByAlias } as unknown as Opts['projects'][number]],
    resolvedImporters,
    workspaceProjectIds: new Set(['packages/pkg-c']),
  })

  const resolvedDep = resolvedImporters['packages/consumer'].directDependencies[0]
  // `applyDedupeMap` normalizes the relative path to forward slashes
  // (normalize-path), so the expected value must be OS-independent.
  expect(resolvedDep.pkgId).toBe('link:../pkg-c')
  expect((resolvedDep as { isLinkedDependency?: boolean }).isLinkedDependency).toBe(true)
})

// Guard for the peer-suffix tolerance above: it must NOT collapse a genuine
// version difference. `isCompatibleAndHasMoreDeps` compares dependency/peer
// sets only, so two versions of a leaf dependency (both empty sets) look
// "compatible"; the `pkgIdWithPatchHash` identity check must keep the injected
// entry as `file:` rather than wrongly deduping it to `link:` and silently
// changing which version the injected package resolves against.
test('injected dependency is NOT deduped when a shared dependency resolves to a different version', () => {
  const nodeId = 1 as NodeId
  const depPath = '@scope/pkg-c@file:packages/pkg-c' as DepPath

  const directNodeIdsByAlias = new Map([['@scope/pkg-c', nodeId]])

  const depGraph = {
    [depPath]: {
      id: 'file:packages/pkg-c' as PkgResolutionId,
      pkgIdWithPatchHash: 'file:packages/pkg-c',
      children: {
        // The injected occurrence resolved this shared leaf dependency to v1.
        'shared-leaf': 'shared-leaf@1.0.0' as DepPath,
      },
    },
    // Two genuinely different versions of the same leaf package — different
    // identities, both with empty dependency/peer sets.
    'shared-leaf@1.0.0': {
      pkgIdWithPatchHash: 'shared-leaf@1.0.0',
      children: {},
      resolvedPeerNames: new Set(),
    },
    'shared-leaf@2.0.0': {
      pkgIdWithPatchHash: 'shared-leaf@2.0.0',
      children: {},
      resolvedPeerNames: new Set(),
    },
  } as unknown as Opts['depGraph']

  const dependenciesByProjectId = {
    'packages/consumer': new Map<string, DepPath>([
      ['@scope/pkg-c', depPath],
    ]),
    'packages/pkg-c': new Map<string, DepPath>([
      // pkg-c's own project resolved the same shared dependency to a DIFFERENT version.
      ['shared-leaf', 'shared-leaf@2.0.0' as DepPath],
    ]),
  }

  const resolvedImporters: Opts['resolvedImporters'] = {
    'packages/consumer': {
      directDependencies: [
        {
          alias: '@scope/pkg-c',
          pkgId: 'file:packages/pkg-c' as PkgResolutionId,
        } as Opts['resolvedImporters'][string]['directDependencies'][number],
      ],
      directNodeIdsByAlias,
      hoistedPeerProviderNodeIds: new Set<NodeId>(),
      linkedDependencies: [],
    },
  }

  dedupeInjectedDeps({
    depGraph,
    dependenciesByProjectId,
    lockfileDir: '/repo',
    pathsByNodeId: new Map([[nodeId, depPath]]),
    projects: [{ id: 'packages/consumer', directNodeIdsByAlias } as unknown as Opts['projects'][number]],
    resolvedImporters,
    workspaceProjectIds: new Set(['packages/pkg-c']),
  })

  const resolvedDep = resolvedImporters['packages/consumer'].directDependencies[0]
  expect(resolvedDep.pkgId).toBe('file:packages/pkg-c')
  expect((resolvedDep as { isLinkedDependency?: boolean }).isLinkedDependency).toBeUndefined()
})
