import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import { fixtures } from '@pnpm/test-fixtures'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import {
  type FastPatchedDependenciesUpdateOptions,
  tryFastUpdatePatchedDependencies,
} from '../../src/install/tryFastUpdatePatchedDependencies.js'
import { testDefaults } from '../utils/index.js'

const f = fixtures(import.meta.dirname)

function trackRequestedPackages (storeController: StoreController): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}

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

test('a dependent reference to a rekeyed package is moved too', () => {
  const lockfile = lockfileWithTransitiveDependency()

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(true)
  expect(lockfile.packages!['bar@2.0.0' as DepPath].dependencies)
    .toStrictEqual({ foo: '1.1.0(patch_hash=foo-hash)' })
  expect(lockfile.importers['.' as ProjectId].dependencies)
    .toStrictEqual({ bar: '2.0.0' })
})

test('a patch for a registry-qualified package falls back to resolution', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.importers['.' as ProjectId].dependencies = { foo: 'work:1.1.0' }
  lockfile.packages = {
    ['foo@work:1.1.0' as DepPath]: { resolution: { integrity: 'sha512-deadbeef' } },
  }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { 'foo@1.1.0': 'foo-hash' },
  }))).toBe(false)
})

test('a bare-name patch that would reach a tarball resolution falls back', () => {
  const lockfile = lockfileWithRegistryDependency()
  lockfile.importers['.' as ProjectId].dependencies = { foo: 'https://example.test/foo.tgz' }
  lockfile.packages = {
    ['foo@https://example.test/foo.tgz' as DepPath]: {
      resolution: { tarball: 'https://example.test/foo.tgz' },
    },
  }

  expect(tryFastUpdatePatchedDependencies(lockfile, updateOptions({
    patchedDependencies: { foo: 'foo-hash' },
  }))).toBe(false)
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

test('adding a patch for a locked package rekeys it without resolution', async () => {
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')
  const manifest: ProjectManifest = { dependencies: { 'is-positive': '1.0.0' } }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const patchedOptions = testDefaults({
    patchedDependencies: { 'is-positive@1.0.0': patchPath },
  })
  const requestedPackages = trackRequestedPackages(patchedOptions.storeController)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, patchedOptions)

  // No registry work, matching the check the catalog and importer fast paths
  // use. It is necessary rather than sufficient — a resolution pass seeded
  // from this unchanged graph would request nothing either — so the proof
  // that resolution is skipped lives in pacquet's `lockfile_resolution_reuse`,
  // where the reporter says so outright.
  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({ 'is-positive@1.0.0': patchFileHash })
  expect(lockfile.snapshots[`is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()
  expect(lockfile.importers['.' as ProjectId].dependencies!['is-positive']).toStrictEqual({
    specifier: '1.0.0',
    version: `1.0.0(patch_hash=${patchFileHash})`,
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')
})

test('the rekeyed lockfile matches what a full resolution writes', async () => {
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')
  const manifest: ProjectManifest = { dependencies: { 'is-positive': '1.0.0' } }

  const rekeyed = prepareEmpty()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ patchedDependencies: { 'is-positive@1.0.0': patchPath } }))
  const rekeyedLockfile = rekeyed.readLockfile()

  const resolved = prepareEmpty()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ patchedDependencies: { 'is-positive@1.0.0': patchPath } }))

  expect(rekeyedLockfile).toStrictEqual(resolved.readLockfile())
})

function updateOptions (
  opts: Partial<FastPatchedDependenciesUpdateOptions> & {
    patchedDependencies: Record<string, string>
  }
): FastPatchedDependenciesUpdateOptions {
  return { allowUnusedPatches: true, ...opts }
}

/** `bar` depends on `foo`, so patching `foo` moves the reference inside `bar`. */
function lockfileWithTransitiveDependency (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { bar: '^2.0.0' },
        dependencies: { bar: '2.0.0' },
      },
    },
    packages: {
      ['bar@2.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { foo: '1.1.0' },
      },
      ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-deadbeef' } },
    },
  }
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
