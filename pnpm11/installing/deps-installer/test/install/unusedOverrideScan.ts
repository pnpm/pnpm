import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ProjectManifest } from '@pnpm/types'

import { findAppliedOverrideSelectorsFromLockfile } from '../../src/install/index.js'

test('version-scoped override that changed the version is not flagged as unused', () => {
  // Override foo@^1: 2.0.0 forces foo from 1.x to 2.0.0.
  // The lockfile shows 2.0.0 (post-override); the scan must NOT
  // compare 2.0.0 against the selector range ^1.
  const lockfile = {
    lockfileVersion: '6.0',
    importers: {
      '.': {
        specifiers: { foo: '^1.0.0' },
        dependencies: { foo: '2.0.0' },
      },
    },
  } as unknown as LockfileObject

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'foo@^1',
      targetPkg: { name: 'foo', bareSpecifier: '^1' },

    },
  ])

  expect(applied.has('foo@^1')).toBe(true)
})

test('override whose target name is absent is flagged as unused', () => {
  const lockfile = {
    lockfileVersion: '6.0',
    importers: {
      '.': {
        specifiers: {},
        dependencies: {},
      },
    },
  } as unknown as LockfileObject

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'foo@^1',
      targetPkg: { name: 'foo', bareSpecifier: '^1' },

    },
  ])

  expect(applied.has('foo@^1')).toBe(false)
})

test('parent-scoped override matches when parent version satisfies range', () => {
  const lockfile = {
    lockfileVersion: '6.0',
    importers: { '.': { specifiers: {}, dependencies: {} } },
    packages: {
      'parent@1.5.0': {
        dependencies: { foo: '2.0.0' },
      },
    },
  } as unknown as LockfileObject

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'parent@^1>foo',
      parentPkg: { name: 'parent', bareSpecifier: '^1' },
      targetPkg: { name: 'foo' },

    },
  ])

  expect(applied.has('parent@^1>foo')).toBe(true)
})

test('parent-scoped override does not match when parent version is absent and range is set', () => {
  // nameVerFromPkgSnapshot returns no version for exotic resolution
  // types. With parentRange set, an absent version must NOT be treated
  // as matching — the package is skipped so the warning path can run.
  const lockfile = {
    lockfileVersion: '6.0',
    importers: { '.': { specifiers: {}, dependencies: {} } },
    packages: {
      'parent': {
        dependencies: { foo: '2.0.0' },
      },
    },
  } as unknown as LockfileObject

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'parent@^1>foo',
      parentPkg: { name: 'parent', bareSpecifier: '^1' },
      targetPkg: { name: 'foo' },

    },
  ])

  expect(applied.has('parent@^1>foo')).toBe(false)
})

test('parent-scoped override matches when parent is a workspace project', () => {
  // Workspace projects appear in lockfile.importers, not lockfile.packages.
  // The scan must check project manifests to detect parent matches.
  const lockfile = {
    lockfileVersion: '6.0',
    importers: {
      '.': {
        specifiers: {},
        dependencies: { foo: '2.0.0' },
      },
    },
  } as unknown as LockfileObject

  const projectManifests: Array<{ importerId: string, manifest: ProjectManifest }> = [
    { importerId: '.', manifest: { name: 'my-app', version: '1.0.0' } },
  ]

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'my-app>foo',
      parentPkg: { name: 'my-app' },
      targetPkg: { name: 'foo' },

    },
  ], projectManifests)

  expect(applied.has('my-app>foo')).toBe(true)
})

test('parent-scoped override with non-semver parent range does not crash', () => {
  // bareSpecifier can be non-semver (e.g. 'latest'); validRange guard
  // prevents semver.satisfies from receiving an invalid range.
  const lockfile = {
    lockfileVersion: '6.0',
    importers: { '.': { specifiers: {}, dependencies: {} } },
    packages: {
      'parent@1.0.0': {
        dependencies: { foo: '2.0.0' },
      },
    },
  } as unknown as LockfileObject

  const applied = findAppliedOverrideSelectorsFromLockfile(lockfile, [
    {
      selector: 'parent@latest>foo',
      parentPkg: { name: 'parent', bareSpecifier: 'latest' },
      targetPkg: { name: 'foo' },

    },
  ])

  // 'latest' is not a valid semver range → no parent matches → unused.
  expect(applied.has('parent@latest>foo')).toBe(false)
})
