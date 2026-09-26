import { expect, test } from '@jest/globals'
import { removeSuffix } from '@pnpm/deps.path'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import type { Project as ImporterProject } from '../../src/install/tryFastUpdateImporters.js'
import { testDefaults } from '../utils/index.js'

const TRANSITIVE_DEP = '@pnpm.e2e/dep-of-pkg-with-1-dep'

test('adding a dependency the lockfile already holds requests no packages', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())
  const locked = lockedVersionOfTransitiveDep(project.readLockfile())

  const options = testDefaults()
  const requestedPackages = trackRequestedPackages(options.storeController)
  const { updatedProject } = await mutateModulesInSingleProject({
    dependencySelectors: [`${TRANSITIVE_DEP}@${locked}`],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  expect(updatedProject.manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    [TRANSITIVE_DEP]: locked,
  })
  expect(project.readLockfile().importers['.'].dependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-1-dep': { specifier: '100.0.0', version: '100.0.0' },
    [TRANSITIVE_DEP]: { specifier: locked, version: locked },
  })
  project.has(TRANSITIVE_DEP)
})

test('an absorbed add writes what a resolution writes', async () => {
  const absorbed = await addToPkgWithOneDep({})
  const resolved = await addToPkgWithOneDep({ forceFullResolution: true })

  expect(absorbed.requestedPackages).toStrictEqual([])
  expect(absorbed.written).toStrictEqual(resolved.written)
})

test('an absorbed add saves the range a resolution saves', async () => {
  // The saved range widens to the locked version rather than staying at the
  // requested bound, which is what the resolver's `calcSpecifier` does too.
  const absorbed = await addToPkgWithOneDep({ range: '^100.0.0' })
  const resolved = await addToPkgWithOneDep({ range: '^100.0.0', forceFullResolution: true })

  expect(absorbed.requestedPackages).toStrictEqual([])
  expect(absorbed.written).toStrictEqual(resolved.written)
})

test('adding a version no locked one satisfies falls back to the resolver', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults())

  const options = testDefaults()
  const requestedPackages = trackRequestedPackages(options.storeController)
  // `@pnpm.e2e/pkg-with-1-dep@100.0.0` declares `^100.0.0`, so no run of it
  // can lock the next major.
  const { updatedProject } = await mutateModulesInSingleProject({
    dependencySelectors: [`${TRANSITIVE_DEP}@101.0.0`],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).not.toStrictEqual([])
  expect(updatedProject.manifest.dependencies?.[TRANSITIVE_DEP]).toBe('101.0.0')
})

test('adding by dist tag falls back to the resolver', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
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
    dependencySelectors: [TRANSITIVE_DEP],
    manifest,
    mutation: 'installSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).not.toStrictEqual([])
})

test('a new dependency is added at the highest locked version satisfying it', async () => {
  const subject = withASecondLockedChild()

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0', child: '^3.0.0' } }),
  ])).toBe(true)
  const importer = subject.importers['.' as ProjectId]
  expect(importer.dependencies).toStrictEqual({ foo: '1.1.0', bar: '2.0.0', child: '3.1.0' })
  expect(importer.specifiers.child).toBe('^3.0.0')
})

test('a new dependency clears the optional flag of what it reaches', async () => {
  const subject = lockfileWithOptionalBar()

  expect(await tryFastUpdateImporters(subject, [
    project({
      dependencies: { foo: '^1.0.0', child: '^3.0.0' },
      optionalDependencies: { bar: '^2.0.0' },
    }),
  ])).toBe(true)
  expect(subject.packages!['bar@2.0.0' as DepPath].optional).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBeUndefined()
})

test('a new optional dependency leaves the flags alone', async () => {
  const subject = lockfileWithOptionalBar()

  expect(await tryFastUpdateImporters(subject, [
    project({
      dependencies: { foo: '^1.0.0' },
      optionalDependencies: { bar: '^2.0.0', child: '^3.0.0' },
    }),
  ])).toBe(true)
  expect(subject.packages!['child@3.0.0' as DepPath].optional).toBe(true)
})

test('a new dependency no locked version satisfies falls back', async () => {
  expect(await tryFastUpdateImporters(lockfile(), [
    project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0', child: '^4.0.0' } }),
  ])).toBe(false)
})

test('a new dependency naming a workspace project falls back', async () => {
  const subject = lockfile()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    projects: [project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0', child: '^3.0.0' } })],
    workspacePackages: new Map([['child', new Map()]]),
    resolutionPicksLowest: false,
  })).toBe(false)
})

test('a new dependency several locked versions satisfy falls back when resolution picks lowest', async () => {
  const subject = withASecondLockedChild()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    projects: [project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0', child: '^3.0.0' } })],
    workspacePackages: new Map(),
    resolutionPicksLowest: true,
  })).toBe(false)
})

test('a new dependency falls back on a lockfile that records publish dates', async () => {
  const subject = lockfile()
  subject.time = {
    'foo@1.1.0': '2020-01-01T00:00:00.000Z',
    'bar@2.0.0': '2020-01-01T00:00:00.000Z',
  }

  expect(await tryFastUpdateImporters(subject, [
    project({ dependencies: { foo: '^1.0.0', bar: '^2.0.0', child: '^3.0.0' } }),
  ])).toBe(false)
})

/**
 * `pnpm add @pnpm.e2e/dep-of-pkg-with-1-dep` onto a project that already
 * depends on `@pnpm.e2e/pkg-with-1-dep@100.0.0`, which locks it transitively.
 * Without a `range`, the selector asks for the exact version that install
 * locked. Runs in a project of its own so the absorbed and the resolved
 * outcome can be compared.
 */
async function addToPkgWithOneDep (
  opts: { range?: string, forceFullResolution?: boolean }
): Promise<{ requestedPackages: string[], written: unknown }> {
  const previousCwd = process.cwd()
  try {
    const project = prepareEmpty()
    const manifest: ProjectManifest = {
      dependencies: {
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      },
    }
    await mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults())
    const range = opts.range ?? lockedVersionOfTransitiveDep(project.readLockfile())
    const options = testDefaults({ forceFullResolution: opts.forceFullResolution })
    const requestedPackages = trackRequestedPackages(options.storeController)
    const { updatedProject } = await mutateModulesInSingleProject({
      dependencySelectors: [`${TRANSITIVE_DEP}@${range}`],
      manifest,
      mutation: 'installSome',
      rootDir: process.cwd() as ProjectRootDir,
    }, options)
    return {
      requestedPackages,
      written: { manifest: updatedProject.manifest, lockfile: project.readLockfile() },
    }
  } finally {
    // `prepareEmpty` moves the process into the new project, so a throw
    // here would otherwise leave every later test in this worker there.
    process.chdir(previousCwd)
  }
}

/**
 * Which version of `@pnpm.e2e/dep-of-pkg-with-1-dep` the install locked. Read
 * rather than assumed: its `latest` dist tag is shared mutable state across the
 * suite, and `^100.0.0` follows it.
 */
function lockedVersionOfTransitiveDep (lockfile: { packages: Record<string, unknown> }): string {
  const prefix = `${TRANSITIVE_DEP}@`
  const depPath = Object.keys(lockfile.packages).find((key) => key.startsWith(prefix))
  expect(depPath).toBeDefined()
  return removeSuffix(depPath!).slice(prefix.length)
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
 * `lockfile()` with a second version of `child` locked, so which end of a range
 * satisfying both is picked becomes observable.
 */
function withASecondLockedChild (): LockfileObject {
  const subject = lockfile()
  subject.packages!['child@3.1.0' as DepPath] = { resolution: { integrity: 'sha512-child-2' } }
  return subject
}

/** `lockfile()` with `bar` — and so `child` under it — reached optionally. */
function lockfileWithOptionalBar (): LockfileObject {
  const subject = lockfile()
  const importer = subject.importers['.' as ProjectId]
  importer.optionalDependencies = { bar: importer.dependencies!.bar }
  delete importer.dependencies!.bar
  subject.packages!['bar@2.0.0' as DepPath].optional = true
  subject.packages!['child@3.0.0' as DepPath].optional = true
  return subject
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
  return tryComposeFastUpdates(lockfile, {
    drift: { importers: true },
    projects,
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
  })
}
