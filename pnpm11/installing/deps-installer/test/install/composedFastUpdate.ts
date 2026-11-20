import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import { testDefaults } from '../utils/index.js'

test('a removal and a widened ignore list are absorbed in one pass', async () => {
  const subject = lockfile()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true, ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [project({ dependencies: { bar: '^2.0.0' }, optionalDependencies: { opt: '^5.0.0' } })],
    ignoredOptionalDependencies: ['opt'],
  })).toBe(true)
  expect(Object.keys(subject.packages!).sort()).toStrictEqual(['bar@2.0.0', 'child@3.0.0'])
  expect(subject.ignoredOptionalDependencies).toStrictEqual(['opt'])
  const importer = subject.importers['.' as ProjectId]
  expect(importer.optionalDependencies).toBeUndefined()
  expect(importer.dependencies).toStrictEqual({ bar: '2.0.0' })
})

test('a group move and a settings change are absorbed in one pass', async () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
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

test('a peer setting is absorbed once the removal drops the last peer dependent', async () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }
  subject.importers['.' as ProjectId].dependencies!['has-peer'] = '6.0.0'
  subject.importers['.' as ProjectId].specifiers['has-peer'] = '^6.0.0'
  subject.packages!['has-peer@6.0.0' as DepPath] = {
    resolution: { integrity: 'sha512-has-peer' },
    peerDependencies: { foo: '^1.0.0' },
  }

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
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

test('a composed update falls back when one of its changes cannot be absorbed', async () => {
  const subject = lockfile()
  subject.settings = { autoInstallPeers: true, excludeLinksFromLockfile: false }

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true, settings: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
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

test('an ignored optional embedded in a surviving peer suffix falls back', async () => {
  const subject = lockfile()
  subject.importers['.' as ProjectId].dependencies!.baz = '4.0.0(opt@5.0.0)'
  subject.importers['.' as ProjectId].specifiers.baz = '^4.0.0'
  subject.packages!['baz@4.0.0(opt@5.0.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz' },
    dependencies: { opt: '5.0.0' },
  }

  expect(await tryComposeFastUpdates(subject, {
    drift: { ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [],
    ignoredOptionalDependencies: ['opt'],
  })).toBe(false)
})

test('an ignored optional removal recomputes the survivors\' optional flags', async () => {
  const subject = lockfile()
  const importer = subject.importers['.' as ProjectId]
  importer.optionalDependencies!.bar = importer.dependencies!.bar
  delete importer.dependencies!.bar
  subject.packages!['bar@2.0.0' as DepPath].optional = true

  expect(await tryComposeFastUpdates(subject, {
    drift: { ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
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

test('an override change and a removal are absorbed in one pass', async () => {
  const project = prepareEmpty()
  await installOverrideFixture(testDefaults())

  const options = testDefaults({ overrides: { '@pnpm.e2e/bar': '100.1.0' } })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/pkg-with-1-dep'],
    manifest: overrideFixtureManifest(),
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  // Only the version the override names is resolved; the removal and the
  // rest of the graph come from the lockfile.
  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/bar'])
  expect(project.readLockfile()).toStrictEqual(await resolveTheSameChanges())
})

test('a catalog change and a removal are absorbed in one pass', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': 'catalog:',
    },
  }
  const options = testDefaults({ catalogs: { default: { 'is-positive': '1.0.0' } } })
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  options.catalogs = { default: { 'is-positive': '>=1.0.0 <2' } }
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/pkg-with-1-dep'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const written = project.readLockfile()
  expect(written.catalogs.default['is-positive']).toStrictEqual({
    specifier: '>=1.0.0 <2',
    version: '1.0.0',
  })
  expect(Object.keys(written.snapshots)).toStrictEqual(['is-positive@1.0.0'])
})

test('an override moving a cataloged package falls back', async () => {
  const subject = lockfile()
  subject.catalogs = { default: { bar: { specifier: '^2.0.0', version: '2.0.0' } } }
  subject.importers['.' as ProjectId].specifiers.bar = 'catalog:'

  // The catalog entry records the version the importer resolved to, so the
  // move would have to carry it along — which only the catalog rewrite does.
  expect(await tryComposeFastUpdates(subject, {
    drift: { overrides: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [],
    overrides: {
      overrides: { bar: '2.1.0' },
      parsedOverrides: [{ selector: 'bar', newBareSpecifier: '2.1.0', targetPkg: { name: 'bar' } }],
      isLockfileUpToDate: async () => true,
      lockfileDir: '/test',
      registriesByScope: { default: 'https://registry.npmjs.org/' },
      requestPackage: (() => {
        throw new Error('the fast path must not resolve anything')
      }) as never,
    },
  })).toBe(false)
})

test('an override on a cataloged package is left to the resolver', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: { '@pnpm.e2e/pkg-with-1-dep': 'catalog:' },
  }
  const options = testDefaults({
    catalogs: { default: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
  })
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  options.overrides = { '@pnpm.e2e/pkg-with-1-dep': '100.1.0' }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  // The override replaces the `catalog:` specifier outright, so the entry is
  // not moved but dropped — a rewrite that only moves packages around cannot
  // express it.
  const written = project.readLockfile()
  expect(written.catalogs).toBeUndefined()
  expect(written.importers['.'].dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toStrictEqual({
    specifier: '100.1.0',
    version: '100.1.0',
  })
})

test('an override change composes when an unchanged override uses the catalog protocol', async () => {
  const project = prepareEmpty()
  const catalogs = { default: { '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0' } }
  const settledOverrides = { '@pnpm.e2e/dep-of-pkg-with-1-dep': 'catalog:' }
  await installOverrideFixture(testDefaults({ catalogs, overrides: settledOverrides }))

  const options = testDefaults({
    catalogs,
    overrides: { ...settledOverrides, '@pnpm.e2e/bar': '100.1.0' },
  })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    manifest: overrideFixtureManifest(),
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  // Only the version the new override names is resolved; the `catalog:`
  // override is compared catalog-resolved, so it shows no drift.
  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/bar'])
  const written = project.readLockfile()
  expect(written.overrides).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    '@pnpm.e2e/bar': '100.1.0',
  })
  expect(Object.keys(written.snapshots)).toContain('@pnpm.e2e/bar@100.1.0')
  expect(Object.keys(written.snapshots)).not.toContain('@pnpm.e2e/bar@100.0.0')
})

/** The two changes a resolution away, in their own project. */
async function resolveTheSameChanges (): Promise<unknown> {
  const previousCwd = process.cwd()
  try {
    const project = prepareEmpty()
    await installOverrideFixture(testDefaults())
    await mutateModulesInSingleProject({
      dependencyNames: ['@pnpm.e2e/pkg-with-1-dep'],
      manifest: overrideFixtureManifest(),
      mutation: 'uninstallSome',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({
      forceFullResolution: true,
      overrides: { '@pnpm.e2e/bar': '100.1.0' },
    }))
    return project.readLockfile()
  } finally {
    // `prepareEmpty` moves the process into the new project, so a throw here
    // would otherwise leave every later test in this worker there.
    process.chdir(previousCwd)
  }
}

/** `@pnpm.e2e/foobarqar` reaches `@pnpm.e2e/bar`, the package the override moves. */
function overrideFixtureManifest (): ProjectManifest {
  return {
    dependencies: {
      '@pnpm.e2e/foobarqar': '1.0.0',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
}

async function installOverrideFixture (options: ReturnType<typeof testDefaults>): Promise<void> {
  await mutateModulesInSingleProject({
    manifest: overrideFixtureManifest(),
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)
}

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
