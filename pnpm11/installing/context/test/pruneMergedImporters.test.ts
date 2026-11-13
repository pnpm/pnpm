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
  const result = maybePruneMergedImporters(lockfile, undefined, [{ id: '.' as ProjectId, manifest: {} }], true)
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

  const pruned = maybePruneMergedImporters(
    mergedLockfile,
    preMergeImporters,
    [{ id: '.' as ProjectId, manifest }],
    true
  )

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

  const pruned = maybePruneMergedImporters(
    mergedLockfile,
    preMergeImporters,
    [{ id: '.' as ProjectId, manifest }],
    false
  )

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

  const pruned = maybePruneMergedImporters(
    mergedLockfile,
    preMergeImporters,
    [{ id: '.' as ProjectId, manifest }],
    true
  )

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
