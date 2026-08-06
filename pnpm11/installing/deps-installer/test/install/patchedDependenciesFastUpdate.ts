import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

import {
  type FastPatchedDependenciesUpdateOptions,
  tryFastUpdatePatchedDependencies,
} from '../../src/install/tryFastUpdatePatchedDependencies.js'

test('a patch that matches no locked package is recorded without resolution', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'bar@2.0.0': 'bar-hash' },
  }))).toBe(true)
  expect(lockfile.patchedDependencies).toStrictEqual({ 'bar@2.0.0': 'bar-hash' })
})

test('a patch that matches a locked package is rekeyed without resolution', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(true)
  expect(Object.keys(lockfile.packages!)).toStrictEqual(['foo@1.1.0(patch_hash=foo-hash)'])
  expect(lockfile.importers['.' as ProjectId].dependencies)
    .toStrictEqual({ foo: '1.1.0(patch_hash=foo-hash)' })
})

test('a bare-name patch key rekeys every version it matches', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { foo: 'foo-hash' },
  }))).toBe(true)
  expect(Object.keys(lockfile.packages!)).toStrictEqual(['foo@1.1.0(patch_hash=foo-hash)'])
})

test('removing a patch that was applied renames the package back', () => {
  const lockfile = lockfileWithPatchedDependency()
  lockfile.patchedDependencies = { 'foo@1.1.0': 'foo-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: {},
  }))).toBe(true)
  expect(Object.keys(lockfile.packages!)).toStrictEqual(['foo@1.1.0'])
  expect(lockfile.importers['.' as ProjectId].dependencies).toStrictEqual({ foo: '1.1.0' })
  expect(lockfile.patchedDependencies).toBeUndefined()
})

test('editing a patch for a locked package rekeys it to the new hash', () => {
  const lockfile = lockfileWithPatchedDependency()
  lockfile.patchedDependencies = { 'foo@1.1.0': 'foo-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'edited-hash' },
  }))).toBe(true)
  expect(Object.keys(lockfile.packages!)).toStrictEqual(['foo@1.1.0(patch_hash=edited-hash)'])
})

test('a package another snapshot reaches as a peer falls back to resolution', () => {
  const lockfile = lockfileWithPeerDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(false)
})

test('a hashed peer suffix falls back to resolution', () => {
  const lockfile = lockfileWithPeerDependency()
  lockfile.packages = {
    ['bar@2.0.0(sha256-abcdef)' as DepPath]: {
      resolution: { integrity: 'sha512-deadbeef' },
      dependencies: { foo: '1.1.0' },
    },
    ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-deadbeef' } },
  }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(false)
})

test('an unused patch falls back to resolution when unused patches are not allowed', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'bar@2.0.0': 'bar-hash' },
    allowUnusedPatches: false,
  }))).toBe(false)
})

test('removing a patch that matched nothing does not need allowUnusedPatches', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.patchedDependencies = { 'bar@2.0.0': 'bar-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: {},
    allowUnusedPatches: false,
  }))).toBe(true)
  expect(lockfile.patchedDependencies).toBeUndefined()
})

test('an unchanged configuration does not claim the install', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.patchedDependencies = { 'bar@2.0.0': 'bar-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'bar@2.0.0': 'bar-hash' },
  }))).toBe(false)
})

function updateOptions (
  opts: Partial<FastPatchedDependenciesUpdateOptions> & {
    patchedDependencies: Record<string, string>
  }
): FastPatchedDependenciesUpdateOptions {
  return { allowUnusedPatches: true, ...opts }
}

/** `bar` reaches `foo` as a peer, so `foo`'s depPath is embedded in `bar`'s key. */
function lockfileWithPeerDependency (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { bar: '^2.0.0' },
        dependencies: { bar: '2.0.0(foo@1.1.0)' },
      },
    },
    packages: {
      ['bar@2.0.0(foo@1.1.0)' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { foo: '1.1.0' },
      },
      ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-deadbeef' } },
    },
  }
}

/** The same graph after a patch for `foo` was applied. */
function lockfileWithPatchedDependency (): LockfileObject {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.importers['.' as ProjectId].dependencies!.foo = '1.1.0(patch_hash=foo-hash)'
  lockfile.packages = {
    ['foo@1.1.0(patch_hash=foo-hash)' as DepPath]: {
      patched: true,
      resolution: { integrity: 'sha512-deadbeef' },
    },
  }
  return lockfile
}

function lockfileWithRegistryDependency (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: '^1.0.0' },
        dependencies: { foo: '1.1.0' },
      },
    },
    packages: {
      ['foo@1.1.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
      },
    },
  }
}
