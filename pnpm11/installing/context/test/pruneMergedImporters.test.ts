/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import type { DepPath, PkgResolutionId, ProjectId, ProjectManifest } from '@pnpm/types'

import { maybePruneMergedImporters } from '../src/pruneMergedImporters.js'

function makeLockfile (importer: ProjectSnapshot, packages: LockfileObject['packages'] = {}): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: importer,
    },
    packages,
  }
}

test('maybePruneMergedImporters returns lockfile unchanged when preMergeImporters is undefined', () => {
  const lockfile = makeLockfile({
    specifiers: { foo: '^1.0.0' },
    dependencies: { foo: '1.0.0' },
    publishDirectory: 'dist',
  })
  const result = maybePruneMergedImporters({
    wantedLockfile: lockfile,
    preMergeImporters: undefined,
    projects: [{ id: '.' as ProjectId, manifest: {} }],
    autoInstallPeers: true,
  })
  expect(result).toBe(lockfile)
  expect(result.importers['.' as ProjectId].publishDirectory).toBe('dist')
})

test('maybePruneMergedImporters reconciles fold-introduced importer metadata against manifest', () => {
  const preMergeImporters: LockfileObject['importers'] = {
    ['.' as ProjectId]: {
      specifiers: { foo: '^1.0.0' },
      dependencies: { foo: '1.0.0' },
      dependenciesMeta: {
        foo: { injected: false },
      },
    },
  }
  const mergedLockfile = makeLockfile({
    specifiers: { foo: '^1.0.0', removed: '^1.0.0' },
    dependencies: { foo: '1.0.0', removed: '1.0.0' },
    publishDirectory: 'dist',
    linkDirectory: false,
    dependenciesMeta: {
      foo: { injected: true, patch: 'patches/foo.patch' },
      removed: { injected: true },
    },
  }, {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
    ['removed@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-removed' } },
  })
  const manifest: ProjectManifest = {
    dependencies: { foo: '^1.0.0' },
    dependenciesMeta: {
      foo: { injected: false },
    },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [{ id: '.' as ProjectId, manifest }],
    autoInstallPeers: true,
  })

  expect(pruned.importers['.' as ProjectId]).toStrictEqual({
    specifiers: { foo: '^1.0.0' },
    dependencies: { foo: '1.0.0' },
    dependenciesMeta: {
      foo: { injected: false },
    },
  })
  expect(Object.keys(pruned.packages ?? {})).toStrictEqual(['foo@1.0.0'])
})

test('maybePruneMergedImporters restores manifest publishConfig overwritten by a branch lockfile and drops empty dependenciesMeta', () => {
  const preMergeImporters: LockfileObject['importers'] = {
    ['.' as ProjectId]: {
      specifiers: {},
      publishDirectory: 'dist',
      linkDirectory: false,
    },
  }
  const mergedLockfile = makeLockfile({
    specifiers: {},
    publishDirectory: 'build',
    dependenciesMeta: {
      stale: { injected: true },
    },
  })
  const manifest: ProjectManifest = {
    publishConfig: {
      directory: 'dist',
      linkDirectory: false,
    },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [{ id: '.' as ProjectId, manifest }],
    autoInstallPeers: false,
  })

  expect(pruned.importers['.' as ProjectId]).toStrictEqual({
    specifiers: {},
    publishDirectory: 'dist',
    linkDirectory: false,
  })
})

test('maybePruneMergedImporters keeps pre-existing metadata drift and auto-installed peers', () => {
  const preMergeImporters: LockfileObject['importers'] = {
    ['.' as ProjectId]: {
      specifiers: { peer: '^1.0.0' },
      dependencies: { peer: '1.0.0' as PkgResolutionId },
      publishDirectory: 'stale-dir',
      linkDirectory: false,
      dependenciesMeta: {
        stale: { injected: true },
      },
    },
  }
  const mergedLockfile = makeLockfile({
    specifiers: { peer: '^1.0.0' },
    dependencies: { peer: '1.0.0' as PkgResolutionId },
    publishDirectory: 'stale-dir',
    linkDirectory: false,
    dependenciesMeta: {
      stale: { injected: true },
    },
  })
  const manifest: ProjectManifest = {
    peerDependencies: { peer: '^1.0.0' },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [{ id: '.' as ProjectId, manifest }],
    autoInstallPeers: true,
  })

  expect(pruned.importers['.' as ProjectId]).toStrictEqual({
    specifiers: { peer: '^1.0.0' },
    dependencies: { peer: '1.0.0' },
    publishDirectory: 'stale-dir',
    linkDirectory: false,
    dependenciesMeta: {
      stale: { injected: true },
    },
  })
})

test('maybePruneMergedImporters restores injected file resolution overwritten with link by a stale branch lockfile', () => {
  const preMergeImporters: LockfileObject['importers'] = {
    ['.' as ProjectId]: {
      specifiers: { '@scope/dep': 'workspace:*' },
      dependencies: { '@scope/dep': 'file:packages/dep' as PkgResolutionId },
      dependenciesMeta: {
        '@scope/dep': { injected: true },
      },
    },
  }
  const mergedLockfile = makeLockfile({
    specifiers: { '@scope/dep': 'workspace:*' },
    dependencies: { '@scope/dep': 'link:packages/dep' as PkgResolutionId },
    dependenciesMeta: {
      '@scope/dep': { injected: false },
    },
  }, {
    ['@scope/dep@file:packages/dep' as DepPath]: {
      resolution: { directory: 'packages/dep', type: 'directory' },
    },
  })
  const manifest: ProjectManifest = {
    dependencies: { '@scope/dep': 'workspace:*' },
    dependenciesMeta: {
      '@scope/dep': { injected: true },
    },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [{ id: '.' as ProjectId, manifest }],
    autoInstallPeers: true,
  })

  expect(pruned.importers['.' as ProjectId]).toStrictEqual({
    specifiers: { '@scope/dep': 'workspace:*' },
    dependencies: { '@scope/dep': 'file:packages/dep' },
    dependenciesMeta: {
      '@scope/dep': { injected: true },
    },
  })
  expect(Object.keys(pruned.packages ?? {})).toStrictEqual(['@scope/dep@file:packages/dep'])
})

test('maybePruneMergedImporters restores link resolution and prunes unreachable file snapshot when injected: true was removed', () => {
  const preMergeImporters: LockfileObject['importers'] = {
    ['.' as ProjectId]: {
      specifiers: { dep: 'workspace:*' },
      dependencies: { dep: 'link:packages/dep' as PkgResolutionId },
    },
  }
  const mergedLockfile = makeLockfile({
    specifiers: { dep: 'workspace:*' },
    dependencies: { dep: 'file:packages/dep' as PkgResolutionId },
    dependenciesMeta: {
      dep: { injected: true },
    },
  }, {
    ['dep@file:packages/dep' as DepPath]: {
      resolution: { directory: 'packages/dep', type: 'directory' },
    },
  })
  const manifest: ProjectManifest = {
    dependencies: { dep: 'workspace:*' },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [{ id: '.' as ProjectId, manifest }],
    autoInstallPeers: true,
  })

  expect(pruned.importers['.' as ProjectId]).toStrictEqual({
    specifiers: { dep: 'workspace:*' },
    dependencies: { dep: 'link:packages/dep' },
  })
  expect(pruned.packages).toBeUndefined()
})

test('maybePruneMergedImporters restores a link to the reconciled publish directory of a workspace dependency', () => {
  const app = 'packages/app' as ProjectId
  const dep = 'packages/dep' as ProjectId
  const preMergeImporters: LockfileObject['importers'] = {
    [app]: {
      specifiers: { dep: 'workspace:*', other: 'workspace:*' },
      dependencies: { dep: 'link:../dep/dist', other: 'link:../other' },
    },
    [dep]: { specifiers: {}, publishDirectory: 'dist' },
  }
  const mergedLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      [app]: {
        specifiers: { dep: 'workspace:*', other: 'workspace:*' },
        dependencies: { dep: 'link:../dep/build', other: 'link:../moved-other' },
      },
      [dep]: { specifiers: {}, publishDirectory: 'build' },
    },
  }

  const pruned = maybePruneMergedImporters({
    wantedLockfile: mergedLockfile,
    preMergeImporters,
    projects: [
      { id: app, manifest: { dependencies: { dep: 'workspace:*', other: 'workspace:*' } } },
      { id: dep, manifest: { name: 'dep', publishConfig: { directory: 'dist' } } },
    ],
    autoInstallPeers: true,
  })

  expect(pruned.importers[dep].publishDirectory).toBe('dist')
  expect(pruned.importers[app].dependencies).toStrictEqual({
    dep: 'link:../dep/dist',
    other: 'link:../moved-other',
  })
})
