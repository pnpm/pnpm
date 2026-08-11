import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import { testDefaults } from '../utils/index.js'

test('a removal and a widened ignore list are absorbed in one pass', () => {
  const subject = lockfile()

  expect(tryComposeFastUpdates(subject, {
    drift: { importers: true, ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    projects: [project({ dependencies: { bar: '^2.0.0' }, optionalDependencies: { opt: '^5.0.0' } })],
    ignoredOptionalDependencies: ['opt'],
  })).toBe(true)
  expect(Object.keys(subject.packages!).sort()).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
  expect(subject.ignoredOptionalDependencies).toStrictEqual(['opt'])
  const importer = subject.importers['.' as ProjectId]
  expect(importer.optionalDependencies).toBeUndefined()
  expect(importer.dependencies).toStrictEqual({ bar: '2.0.0' })
})

test('a group move and a settings change are absorbed in one pass', () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }

  expect(tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    projects: [project({
      devDependencies: { foo: '^1.0.0' },
      dependencies: { bar: '^2.0.0' },
      optionalDependencies: { opt: '^5.0.0' },
    })],
    settings: {
      changedSettings: ['settings.autoInstallPeers'],
      projects: [project({ devDependencies: { foo: '^1.0.0' } })],
      settings: { autoInstallPeers: false, excludeLinksFromLockfile: false },
      workspacePackages: new Map(),
    },
  })).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.devDependencies).toStrictEqual({ foo: '1.1.0' })
  expect(subject.settings).toStrictEqual({ autoInstallPeers: false, excludeLinksFromLockfile: false })
})

test('a peer setting is absorbed once the removal drops the last peer dependent', () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }
  subject.importers['.' as ProjectId].dependencies!['has-peer'] = '6.0.0'
  subject.importers['.' as ProjectId].specifiers['has-peer'] = '^6.0.0'
  subject.packages!['has-peer@6.0.0' as DepPath] = {
    resolution: { integrity: 'sha512-has-peer' },
    peerDependencies: { foo: '^1.0.0' },
  }

  expect(tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    projects: [project({
      dependencies: { foo: '^1.0.0', bar: '^2.0.0' },
      optionalDependencies: { opt: '^5.0.0' },
    })],
    settings: {
      changedSettings: ['settings.autoInstallPeers'],
      projects: [],
      settings: { autoInstallPeers: false, excludeLinksFromLockfile: false },
      workspacePackages: new Map(),
    },
  })).toBe(true)
  expect(subject.settings).toStrictEqual({ autoInstallPeers: false, excludeLinksFromLockfile: false })
  expect(subject.packages!['has-peer@6.0.0' as DepPath]).toBeUndefined()
})

test('a composed update falls back when one of its changes cannot be absorbed', () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }

  expect(tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    projects: [project({
      dependencies: { foo: '^9.0.0', bar: '^2.0.0' },
      optionalDependencies: { opt: '^5.0.0' },
    })],
    settings: {
      changedSettings: ['settings.autoInstallPeers'],
      projects: [],
      settings: { autoInstallPeers: false, excludeLinksFromLockfile: false },
      workspacePackages: new Map(),
    },
  })).toBe(false)
})

test('an ignored optional embedded in a surviving peer suffix falls back', () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(opt@5.0.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(opt@5.0.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { opt: '5.0.0' },
  }

  expect(tryComposeFastUpdates(subject, {
    drift: { ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    projects: [],
    ignoredOptionalDependencies: ['opt'],
  })).toBe(false)
})

test('an ignored optional removal recomputes the survivors\' optional flags', () => {
  const subject = lockfile()
  const importer = subject.importers['.' as ProjectId]
  importer.optionalDependencies!.bar = importer.dependencies!.bar
  delete importer.dependencies!.bar
  subject.packages!['bar@2.0.0' as DepPath].optional = true

  expect(tryComposeFastUpdates(subject, {
    drift: { ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    projects: [],
    ignoredOptionalDependencies: ['bar'],
  })).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBe(true)
})

test('an install with combined manifest and ignore-list drift requests no packages', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const options = testDefaults({ ignoredOptionalDependencies: ['@pnpm.e2e/pkg-with-good-optional'] })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    manifest: {
      dependencies: { 'is-positive': '1.0.0' },
      optionalDependencies: { '@pnpm.e2e/pkg-with-good-optional': '1.0.0' },
    },
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const written = project.readLockfile()
  expect(written.ignoredOptionalDependencies).toStrictEqual(['@pnpm.e2e/pkg-with-good-optional'])
  expect(Object.keys(written.snapshots).some((depPath) =>
    depPath.startsWith('@pnpm.e2e/pkg-with-1-dep@') ||
    depPath.startsWith('@pnpm.e2e/pkg-with-good-optional@'))
  ).toBe(false)
  expect(written.importers['.']).toStrictEqual({
    dependencies: {
      'is-positive': { specifier: '1.0.0', version: '1.0.0' },
    },
  })
  project.has('is-positive')
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')
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

/**
 * `foo` and `bar` are prod dependencies (`bar` reaching `child`), and `opt`
 * is an optional dependency reaching the same `child`.
 */
function lockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { foo: '^1.0.0', bar: '^2.0.0', opt: '^5.0.0' },
        dependencies: { foo: '1.1.0', bar: '2.0.0' },
        optionalDependencies: { opt: '5.0.0' },
      },
    },
    packages: {
      ['foo@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
      ['bar@2.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-bar' },
        dependencies: { child: '3.0.0' },
      },
      ['child@3.0.0' as DepPath]: { resolution: { integrity: 'sha512-child' } },
      ['opt@5.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-opt' },
        optional: true,
        dependencies: { child: '3.0.0' },
      },
    },
  }
}
