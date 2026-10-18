import { expect, test } from '@jest/globals'
import type { DepPath, PkgResolutionId } from '@pnpm/types'

import { dedupeInjectedDeps, type DedupeInjectedDepsOptions } from '../lib/dedupeInjectedDeps.js'
import type { NodeId } from '../lib/nextNodeId.js'
import type { PartialResolvedPackage } from '../lib/resolvePeers.js'

type Opts = DedupeInjectedDepsOptions<PartialResolvedPackage>

// Regression test for https://github.com/pnpm/pnpm/issues/10433: an injected
// workspace dep must still dedupe to `link:` when an unrelated shared dep
// (debug) resolves peer-suffixed for the target project but peer-free for the
// injected occurrence -- both are valid resolutions of the same package.
test('injected dependency dedupes to link: even when an unrelated shared dependency has a peer suffix on only one side', () => {
  const nodeId = 1 as NodeId
  const depPath = '@scope/pkg-c@file:packages/pkg-c(@scope/pkg-a@file:packages/pkg-a)' as DepPath

  const directNodeIdsByAlias = new Map([['@scope/pkg-c', nodeId]])

  const depGraph = {
    [depPath]: {
      id: 'file:packages/pkg-c' as PkgResolutionId,
      pkgIdWithPatchHash: 'file:packages/pkg-c',
      children: {
        debug: 'debug@4.4.3' as DepPath,
        '@scope/pkg-a': '@scope/pkg-a@file:packages/pkg-a' as DepPath,
      },
    },
    // debug's two resolutions share one pkgIdWithPatchHash, differing only by
    // peer suffix.
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
      // Target project's debug is pinned to its optional peer by the lockfile.
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
    wantedLockfile: { importers: {} },
    workspaceProjectIds: new Set(['packages/pkg-c']),
  })

  const resolvedDep = resolvedImporters['packages/consumer'].directDependencies[0]
  // `applyDedupeMap` normalizes the relative path to forward slashes
  // (normalize-path), so the expected value must be OS-independent.
  expect(resolvedDep.pkgId).toBe('link:../pkg-c')
  expect((resolvedDep as { isLinkedDependency?: boolean }).isLinkedDependency).toBe(true)
})

// The identity guard must reject a genuine version difference: two leaf
// versions have equal (empty) dependency sets, so isCompatibleAndHasMoreDeps
// alone would treat them as interchangeable.
test('injected dependency is NOT deduped when a shared dependency resolves to a different version', () => {
  const nodeId = 1 as NodeId
  const depPath = '@scope/pkg-c@file:packages/pkg-c' as DepPath

  const directNodeIdsByAlias = new Map([['@scope/pkg-c', nodeId]])

  const depGraph = {
    [depPath]: {
      id: 'file:packages/pkg-c' as PkgResolutionId,
      pkgIdWithPatchHash: 'file:packages/pkg-c',
      children: {
        'shared-leaf': 'shared-leaf@1.0.0' as DepPath,
      },
    },
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
    wantedLockfile: { importers: {} },
    workspaceProjectIds: new Set(['packages/pkg-c']),
  })

  const resolvedDep = resolvedImporters['packages/consumer'].directDependencies[0]
  expect(resolvedDep.pkgId).toBe('file:packages/pkg-c')
  expect((resolvedDep as { isLinkedDependency?: boolean }).isLinkedDependency).toBeUndefined()
})

// https://github.com/pnpm/pnpm/issues/17094: the resolver skips a dependency
// that is installed and not updated, so the target project's resolved
// dependencies can miss it. Its locked entry stands in for it.
test.each([
  { locked: true, expected: 'link:../packages/provider' },
  { locked: false, expected: 'file:packages/provider' },
])('an injected dependency reads a child the install skipped from the lockfile (locked: $locked)', ({ locked, expected }) => {
  const nodeId = 1 as NodeId
  const depPath = 'provider@file:packages/provider' as DepPath
  const directNodeIdsByAlias = new Map([['provider', nodeId]])
  const depGraph = {
    [depPath]: {
      id: 'file:packages/provider' as PkgResolutionId,
      pkgIdWithPatchHash: 'file:packages/provider',
      children: { 'peer-c': 'peer-c@1.0.0' as DepPath },
    },
    'peer-c@1.0.0': { pkgIdWithPatchHash: 'peer-c@1.0.0', children: {} },
  } as unknown as Opts['depGraph']
  const resolvedImporters: Opts['resolvedImporters'] = {
    app: {
      directDependencies: [
        { alias: 'provider', pkgId: 'file:packages/provider' as PkgResolutionId } as Opts['resolvedImporters'][string]['directDependencies'][number],
      ],
      directNodeIdsByAlias,
      hoistedPeerProviderNodeIds: new Set<NodeId>(),
      linkedDependencies: [],
    },
  }
  const providerImporter = {
    specifiers: { 'peer-c': '1.0.0' },
    ...(locked ? { dependencies: { 'peer-c': '1.0.0' } } : {}),
  }

  dedupeInjectedDeps({
    depGraph,
    // The install resolved none of the provider's dependencies.
    dependenciesByProjectId: { app: new Map([['provider', depPath]]), 'packages/provider': new Map() },
    lockfileDir: '/repo',
    pathsByNodeId: new Map([[nodeId, depPath]]),
    projects: [{ id: 'app', directNodeIdsByAlias } as unknown as Opts['projects'][number]],
    resolvedImporters,
    wantedLockfile: { importers: { 'packages/provider': providerImporter } } as unknown as Opts['wantedLockfile'],
    workspaceProjectIds: new Set(['packages/provider']),
  })

  expect(resolvedImporters.app.directDependencies[0].pkgId).toBe(expected)
})
