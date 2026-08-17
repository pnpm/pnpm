import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import { fixtures } from '@pnpm/test-fixtures'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
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

test('a removal that orphans an allowed unused patch still reports it', async () => {
  prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')
  const manifest: ProjectManifest = {
    dependencies: { 'is-positive': '1.0.0', '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
  }
  const options = testDefaults({
    allowUnusedPatches: true,
    patchedDependencies: { 'is-positive@1.0.0': patchPath },
  })
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const reporter = jest.fn()
  const removalOptions = testDefaults({
    allowUnusedPatches: true,
    patchedDependencies: { 'is-positive@1.0.0': patchPath },
    reporter,
  })
  const requestedPackages = trackRequestedPackages(removalOptions.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, removalOptions)

  // The rewrite keeps the fast path, and still says what the resolution it
  // replaced would have said.
  expect(requestedPackages).toStrictEqual([])
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'warn',
    message: 'The following patches were not used: is-positive@1.0.0',
  }))
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

/**
 * `parent` is the only thing that reaches the patched `victim`, so dropping it
 * leaves the patch with nothing to apply to — `ERR_PNPM_UNUSED_PATCH`, which
 * only a resolution raises.
 */
function lockfileWithAPatchedTransitiveDependency (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    patchedDependencies: { 'victim@1.0.0': 'victim-hash' },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { parent: '^1.0.0', keep: '^2.0.0' },
        dependencies: { parent: '1.0.0', keep: '2.0.0' },
      },
    },
    packages: {
      ['parent@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-parent' },
        dependencies: { victim: '1.0.0(patch_hash=victim-hash)' },
      },
      ['victim@1.0.0(patch_hash=victim-hash)' as DepPath]: { resolution: { integrity: 'sha512-victim' } },
      ['keep@2.0.0' as DepPath]: { resolution: { integrity: 'sha512-keep' } },
    },
  } as unknown as LockfileObject
}

const patchedTransitive: FastPatchedDependenciesUpdateOptions = {
  patchedDependencies: { 'victim@1.0.0': 'victim-hash' },
  allowUnusedPatches: false,
}

test('a manifest removal that leaves a patch unused falls back', async () => {
  expect(await tryComposeFastUpdates(lockfileWithAPatchedTransitiveDependency(), {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{ id: '.' as ProjectId, manifest: { dependencies: { keep: '^2.0.0' } } as ProjectManifest }],
    patchedDependencies: patchedTransitive,
  })).toBe(false)
})

test('a removal override that leaves a patch unused falls back', async () => {
  expect(await tryComposeFastUpdates(lockfileWithAPatchedTransitiveDependency(), {
    drift: { overrides: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [],
    patchedDependencies: patchedTransitive,
    overrides: {
      overrides: { victim: '-' },
      parsedOverrides: [{ selector: 'victim', newBareSpecifier: '-', targetPkg: { name: 'victim' } }],
      isLockfileUpToDate: async () => true,
      lockfileDir: '/test',
      registriesByScope: { default: 'https://registry.npmjs.org/' },
      requestPackage: (() => {
        throw new Error('a removal resolves nothing')
      }) as never,
    },
  })).toBe(false)
})

test('a removal that keeps the patch applied is still absorbed', async () => {
  const subject = lockfileWithAPatchedTransitiveDependency()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{ id: '.' as ProjectId, manifest: { dependencies: { parent: '^1.0.0' } } as ProjectManifest }],
    patchedDependencies: patchedTransitive,
  })).toBe(true)
  expect(Object.keys(subject.packages!).sort())
    .toStrictEqual(['parent@1.0.0', 'victim@1.0.0(patch_hash=victim-hash)'])
})

test('a removal that leaves a patch unused is absorbed under allowUnusedPatches', async () => {
  expect(await tryComposeFastUpdates(lockfileWithAPatchedTransitiveDependency(), {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{ id: '.' as ProjectId, manifest: { dependencies: { keep: '^2.0.0' } } as ProjectManifest }],
    patchedDependencies: { ...patchedTransitive, allowUnusedPatches: true },
  })).toBe(true)
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
