import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'

test('removing a deduplicated peer dependent needs resolution for its remaining consumers', async () => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { consumer: '1.0.0', provider: '2.0.0' },
        dependencies: { consumer: '1.0.0(provider@2.0.0)', provider: '2.0.0' },
      },
      ['app' as ProjectId]: {
        specifiers: { consumer: '1.0.0' },
        dependencies: { consumer: '1.0.0(provider@2.0.0)' },
      },
    },
    packages: {
      ['consumer@1.0.0(provider@2.0.0)' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
        peerDependencies: { provider: '^2.0.0' },
        peerDependenciesMeta: { provider: { optional: true } },
        optionalDependencies: { provider: '2.0.0' },
      },
      ['provider@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
    },
  }
  const accepted = await tryComposeFastUpdates(lockfile, {
    drift: { importers: true },
    projects: [
      { id: '.' as ProjectId, manifest: { dependencies: { provider: '2.0.0' } } },
      { id: 'app' as ProjectId, manifest: { dependencies: { consumer: '1.0.0' } } },
    ],
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
  })
  expect(accepted).toBe(false)
})

test.each([
  ['consumer', true],
  ['wrapper', true],
  ['consumer', false],
  ['wrapper', false],
] as const)('adding %s with declared peers=%s reuses only peer-free subtrees', async (dependency, hasPeers) => {
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: { specifiers: { provider: '2.0.0' }, dependencies: { provider: '2.0.0' } },
      ['app' as ProjectId]: { specifiers: { wrapper: '1.0.0' }, dependencies: { wrapper: '1.0.0' } },
    },
    packages: {
      ['wrapper@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
        dependencies: { consumer: '1.0.0' },
      },
      ['consumer@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
        peerDependencies: hasPeers ? { provider: '^2.0.0' } : undefined,
        peerDependenciesMeta: hasPeers ? { provider: { optional: true } } : undefined,
        dependencies: { provider: '2.0.0' },
      },
      ['provider@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
    },
  }
  const accepted = await tryComposeFastUpdates(lockfile, {
    drift: { importers: true },
    projects: [
      { id: '.' as ProjectId, manifest: { dependencies: { provider: '2.0.0', [dependency]: '1.0.0' } } },
      { id: 'app' as ProjectId, manifest: { dependencies: { wrapper: '1.0.0' } } },
    ],
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
  })
  expect(accepted).toBe(!hasPeers)
})

test.each([false, true])('removing a provider source with declared peers=%s resolves its optional consumers', async (hasPeers) => {
  const requiredReference = hasPeers ? '1.0.0(provider@1.0.0)' : '1.0.0'
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: { specifiers: { optional: '1.0.0' }, dependencies: { optional: '1.0.0(provider@1.0.0)' } },
      ['app' as ProjectId]: { specifiers: { required: '1.0.0' }, dependencies: { required: requiredReference } },
    },
    packages: {
      ['optional@1.0.0(provider@1.0.0)' as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
        peerDependencies: { provider: '^1.0.0' },
        peerDependenciesMeta: { provider: { optional: true } },
        optionalDependencies: { provider: '1.0.0' },
      },
      [`required@${requiredReference}` as DepPath]: {
        resolution: { integrity: 'sha512-AAAA' },
        peerDependencies: hasPeers ? { provider: '^1.0.0' } : undefined,
        dependencies: { provider: '1.0.0' },
      },
      ['provider@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
    },
  }
  expect(await tryComposeFastUpdates(lockfile, {
    drift: { importers: true },
    projects: [
      { id: '.' as ProjectId, manifest: { dependencies: { optional: '1.0.0' } } },
      { id: 'app' as ProjectId, manifest: {} },
    ],
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
  })).toBe(false)
})
