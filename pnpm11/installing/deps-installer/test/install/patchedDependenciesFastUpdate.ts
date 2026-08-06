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

test('a patch that matches a locked package falls back to resolution', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(false)
  expect(lockfile.patchedDependencies).toBeUndefined()
})

test('a bare-name patch key that matches a locked package falls back to resolution', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { foo: 'foo-hash' },
  }))).toBe(false)
})

test('a range patch key that matches a locked package falls back to resolution', () => {
  const lockfile = lockfileWithRegistryDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@^1.0.0': 'foo-hash' },
  }))).toBe(false)
})

test('removing a patch that was applied to a locked package falls back to resolution', () => {
  const lockfile = lockfileWithPatchedDependency()
  lockfile.patchedDependencies = { 'foo@1.1.0': 'foo-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: {},
  }))).toBe(false)
  expect(lockfile.patchedDependencies).toStrictEqual({ 'foo@1.1.0': 'foo-hash' })
})

test('an editing of a patch for a locked package falls back to resolution', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.patchedDependencies = { 'foo@1.1.0': 'stale-hash' }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(false)
  expect(lockfile.patchedDependencies).toStrictEqual({ 'foo@1.1.0': 'stale-hash' })
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
