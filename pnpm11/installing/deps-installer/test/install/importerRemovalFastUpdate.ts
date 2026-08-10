import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { clone } from 'ramda'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import {
  hasChangedProjectSpecifiers,
  type Project,
} from '../../src/install/tryFastUpdateImporters.js'
import { tryFastUpdateLockfile } from '../../src/install/tryFastUpdateLockfile.js'
import { testDefaults } from '../utils/index.js'

test('a dependency the manifest dropped is noticed as a change', () => {
  expect(hasChangedProjectSpecifiers(lockfile(), [project({ bar: '^2.0.0' })])).toBe(true)
})

test('a dropped dependency is removed and its subtree pruned', () => {
  const subject = lockfile()

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.specifiers).toStrictEqual({ bar: '^2.0.0' })
  expect(importer.dependencies).toStrictEqual({ bar: '2.0.0' })
  expect(Object.keys(subject.packages!).sort()).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
})

test('dropping a dependency another package resolves as a peer falls back without mutating the lockfile', async () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(foo@1.1.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(foo@1.1.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { foo: '1.1.0' },
  }
  const before = clone(subject)

  expect(await tryFastUpdateLockfile(subject, {
    update: (candidate) => tryFastUpdateImporters(candidate, [project({ bar: '^2.0.0', baz: '^4.0.0' })]),
    isLockfileUpToDate: async () => true,
  })).toBe(false)
  expect(subject).toStrictEqual(before)
})

test('a lockfile key the update deletes is gone after the commit', async () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }

  expect(await tryFastUpdateLockfile(subject, {
    update: (candidate) => {
      delete candidate.catalogs
      return true
    },
    isLockfileUpToDate: async () => true,
  })).toBe(true)
  expect(subject.catalogs).toBeUndefined()
})

test('dropping a dependency together with its peer-dependent succeeds', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(foo@1.1.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(foo@1.1.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { foo: '1.1.0' },
  }

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  expect(Object.keys(subject.packages!)).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
})

test('dropping a dependency when a surviving peer suffix is hashed falls back', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(sha256-abcdef)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(sha256-abcdef)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
  }

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0', baz: '^4.0.0' })])).toBe(false)
})

test('a dependency group emptied by the removal is deleted', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].devDependencies = { qux: '5.0.0' }
  subject.importers['.' as ProjectId].specifiers.qux = '^5.0.0'
  subject.packages!['qux@5.0.0' as DepPath] = { resolution: { integrity: 'sha512-qux' } }

  expect(tryFastUpdateImporters(subject, [project({ foo: '^1.0.0', bar: '^2.0.0' })])).toBe(true)
  expect(subject.importers['.' as ProjectId].devDependencies).toBeUndefined()
})

test('a catalog entry whose last referent is dropped is pruned', () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }
  subject.importers['.' as ProjectId].specifiers.foo = 'catalog:'

  expect(tryFastUpdateImporters(subject, [project({ bar: '^2.0.0' })])).toBe(true)
  expect(subject.catalogs).toBeUndefined()
})

test('a catalog entry referenced with the catalog:default spelling is kept', () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }
  subject.importers['.' as ProjectId].specifiers.foo = 'catalog:default'
  subject.importers['pkg-a' as ProjectId] = {
    specifiers: { foo: 'catalog:default', bar: '^2.0.0' },
    dependencies: { foo: '1.1.0', bar: '2.0.0' },
  }

  expect(tryFastUpdateImporters(subject, [
    project({ bar: '^2.0.0' }),
    { id: 'pkg-a' as ProjectId, manifest: { dependencies: { foo: 'catalog:default', bar: '^2.0.0' } } as ProjectManifest },
  ])).toBe(true)
  expect(subject.catalogs).toStrictEqual({ default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } })
})

test('a catalog entry another importer references is kept', () => {
  const subject = lockfile()
  subject.catalogs = { default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } }
  subject.importers['.' as ProjectId].specifiers.foo = 'catalog:'
  subject.importers['pkg-a' as ProjectId] = {
    specifiers: { foo: 'catalog:' },
    dependencies: { foo: '1.1.0' },
  }

  expect(tryFastUpdateImporters(subject, [
    project({ bar: '^2.0.0' }),
    { id: 'pkg-a' as ProjectId, manifest: { dependencies: { foo: 'catalog:' } } as ProjectManifest },
  ])).toBe(true)
  expect(subject.catalogs).toStrictEqual({ default: { foo: { specifier: '^1.0.0', version: '1.1.0' } } })
})

test('pnpm remove drops the dependency without requesting packages', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const options = testDefaults()
  const requestedPackages = trackRequestedPackages(options.storeController)
  const { updatedProject } = await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  expect(updatedProject.manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
  })
  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages).some((depPath) => depPath.startsWith('is-positive@'))).toBe(false)
  expect(lockfile.importers['.']).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': { specifier: '100.0.0', version: '100.0.0' },
    },
  })
  project.hasNot('is-positive')
  project.has('@pnpm.e2e/pkg-with-1-dep')
})

function trackRequestedPackages (storeController: StoreController): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}

function project (dependencies: Record<string, string>) {
  return {
    id: '.' as ProjectId,
    manifest: { dependencies } as ProjectManifest,
  }
}

function lockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: '^1.0.0', bar: '^2.0.0' },
        dependencies: { foo: '1.1.0', bar: '2.0.0' },
      },
    },
    packages: {
      ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
      ['bar@2.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-bar' },
        dependencies: { child: '3.0.0' },
      },
      ['child@3.0.0' as DepPath]: { resolution: { integrity: 'sha512-child' } },
    },
  }
}

test('removing the last dependency drops the packages section from the lockfile', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const options = testDefaults()
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toBeUndefined()
  expect(lockfile.snapshots).toBeUndefined()
  project.hasNot('is-positive')
})

test('removing the last catalog referent drops the catalogs section from the lockfile', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      'is-positive': 'catalog:',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const options = testDefaults()
  options.catalogs = { default: { 'is-positive': '1.0.0' } }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)
  expect(project.readLockfile().catalogs).toBeDefined()

  const removeOptions = testDefaults()
  removeOptions.catalogs = { default: { 'is-positive': '1.0.0' } }
  const requestedPackages = trackRequestedPackages(removeOptions.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, removeOptions)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.catalogs).toBeUndefined()
  expect(lockfile.importers['.'].dependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-1-dep': { specifier: '100.0.0', version: '100.0.0' },
  })
})
/** The composed pipeline restricted to manifest drift. */
function tryFastUpdateImporters (lockfile: LockfileObject, projects: Project[]): boolean {
  return tryComposeFastUpdates(lockfile, { drift: { importers: true }, projects })
}
