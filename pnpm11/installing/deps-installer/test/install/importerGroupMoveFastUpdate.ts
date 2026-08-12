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
  type Project as ImporterProject,
} from '../../src/install/tryFastUpdateImporters.js'
import { testDefaults } from '../utils/index.js'

test('a dependency moved to another group is noticed as a change', () => {
  expect(hasChangedProjectSpecifiers(lockfile(), [
    project({ dependencies: { foo: '^1.0.0' }, devDependencies: { bar: '^2.0.0' } }),
  ])).toBe(true)
})

test('a dependency in its recorded group is not a change', () => {
  expect(hasChangedProjectSpecifiers(lockfile(), [
    project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0' } }),
  ])).toBe(false)
})

test('a move between prod and dev only edits the importer', async () => {
  const subject = lockfile()
  const packagesBefore = clone(subject.packages)

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0' }, devDependencies: { bar: '^2.0.0' } }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.dependencies).toStrictEqual({ foo: '1.1.0' })
  expect(importer.devDependencies).toStrictEqual({ bar: '2.0.0' })
  expect(importer.specifiers).toStrictEqual({ foo: '^1.0.0', bar: '^2.0.0' })
  expect(subject.packages).toStrictEqual(packagesBefore)
})

test('a move into optionalDependencies marks the subtree optional', async () => {
  const subject = lockfile()

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0' }, optionalDependencies: { bar: '^2.0.0' } }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.dependencies).toStrictEqual({ foo: '1.1.0' })
  expect(importer.optionalDependencies).toStrictEqual({ bar: '2.0.0' })
  expect(subject.packages!['bar@2.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['foo@1.1.0' as DepPath].optional).toBeUndefined()
})

test('a move out of optionalDependencies clears the subtree flags', async () => {
  const subject = lockfile()
  const importer = subject.importers['.' as ProjectId]
  importer.optionalDependencies = { bar: importer.dependencies!.bar }
  importer.dependencies = { foo: importer.dependencies!.foo }
  subject.packages!['bar@2.0.0' as DepPath].optional = true
  subject.packages!['child@3.0.0' as DepPath].optional = true

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0' } }),
  ])).toBe(true)
  expect(subject.importers['.' as ProjectId].optionalDependencies).toBeUndefined()
  expect(subject.importers['.' as ProjectId].dependencies).toStrictEqual({ foo: '1.1.0', bar: '2.0.0' })
  expect(subject.packages!['bar@2.0.0' as DepPath].optional).toBeUndefined()
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBeUndefined()
})

test('a subtree package another prod dependency reaches stays non-optional', async () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.parent = '4.0.0'
  subject.importers['.' as ProjectId].specifiers.parent = '^4.0.0'
  subject.packages!['parent@4.0.0' as DepPath] = {
    resolution: { integrity: 'sha512-parent' },
    dependencies: { child: '3.0.0' },
  }

  expect(await tryFastUpdateImporters(subject, [
    project({
      dependencies: { foo: '^1.0.0', parent: '^4.0.0' },
      optionalDependencies: { bar: '^2.0.0' },
    }),
  ])).toBe(true)
  expect(subject.packages!['bar@2.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBeUndefined()
})

test('an alias in both prod and optional groups is recorded as optional', async () => {
  const subject = lockfile()

  expect(await tryFastUpdateImporters(subject, [
    project({
      dependencies: { foo: '^1.0.0', bar: '^2.0.0' },
      optionalDependencies: { bar: '^2.0.0' },
    }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.dependencies).toStrictEqual({ foo: '1.1.0' })
  expect(importer.optionalDependencies).toStrictEqual({ bar: '2.0.0' })
})

test('several dependencies move between groups in one pass', async () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].devDependencies = { qux: '5.0.0' }
  subject.importers['.' as ProjectId].specifiers.qux = '^5.0.0'
  subject.packages!['qux@5.0.0' as DepPath] = { resolution: { integrity: 'sha512-qux' } }

  expect(await tryFastUpdateImporters(subject, [
    project({
      dependencies: { qux: '^5.0.0' },
      devDependencies: { foo: '^1.0.0' },
      optionalDependencies: { bar: '^2.0.0' },
    }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.dependencies).toStrictEqual({ qux: '5.0.0' })
  expect(importer.devDependencies).toStrictEqual({ foo: '1.1.0' })
  expect(importer.optionalDependencies).toStrictEqual({ bar: '2.0.0' })
  expect(subject.packages!['bar@2.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['qux@5.0.0' as DepPath].optional).toBeUndefined()
  expect(subject.packages!['foo@1.1.0' as DepPath].optional).toBeUndefined()
})

test('a group move rides along with a satisfied range change', async () => {
  const subject = lockfile()

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0' }, devDependencies: { bar: '^2.0.0 <3.0.0' } }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.devDependencies).toStrictEqual({ bar: '2.0.0' })
  expect(importer.specifiers.bar).toBe('^2.0.0 <3.0.0')
})

test('moving a dependency between groups requests no packages', async () => {
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
  await mutateModulesInSingleProject({
    manifest: {
      dependencies: { 'is-positive': '1.0.0' },
      optionalDependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.']).toStrictEqual({
    dependencies: {
      'is-positive': { specifier: '1.0.0', version: '1.0.0' },
    },
    optionalDependencies: {
      '@pnpm.e2e/pkg-with-1-dep': { specifier: '100.0.0', version: '100.0.0' },
    },
  })
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.0.0'].optional).toBe(true)
  const depOfPkgSnapshot = Object.entries(lockfile.snapshots)
    .find(([depPath]) => depPath.startsWith('@pnpm.e2e/dep-of-pkg-with-1-dep@'))?.[1]
  expect(depOfPkgSnapshot?.optional).toBe(true)
  project.has('@pnpm.e2e/pkg-with-1-dep')
  project.has('is-positive')
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

function project (manifest: Pick<ProjectManifest, 'dependencies' | 'devDependencies' | 'optionalDependencies'>) {
  return {
    id: '.' as ProjectId,
    manifest: manifest as ProjectManifest,
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
/** The composed pipeline restricted to manifest drift. */
async function tryFastUpdateImporters (lockfile: LockfileObject, projects: ImporterProject[]): Promise<boolean> {
  return tryComposeFastUpdates(lockfile, { drift: { importers: true }, projects, workspacePackages: new Map(), resolutionPicksLowest: false })
}
