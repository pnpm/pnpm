import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { mutateModules } from '@pnpm/installing.deps-installer'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import { testDefaults } from '../utils/index.js'

test('adding a workspace package whose dependencies are locked resolves nothing', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { 'is-positive': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1', 'project-2'])
  expect(lockfile.importers['project-2' as ProjectId].dependencies).toStrictEqual({
    'is-positive': { specifier: '1.0.0', version: '1.0.0' },
  })
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['is-positive@1.0.0'])
})

test('a new project reuses the highest locked version its range admits', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install(TWO_LOCKED_VERSIONS)

  const requestedPackages = await install([
    ...TWO_LOCKED_VERSIONS,
    { name: 'project-2', devDependencies: { '@pnpm.e2e/foo': '^1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(lockfile.importers['project-2' as ProjectId].devDependencies).toStrictEqual({
    '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.2.0' },
  })
  expect(Object.keys(lockfile.packages ?? {}).sort()).toStrictEqual([
    '@pnpm.e2e/foo@1.0.0',
    '@pnpm.e2e/foo@1.2.0',
  ])
})

test('a new project with a dependency no locked version satisfies falls back to the resolver', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { 'is-negative': '1.0.0' } },
  ])

  expect(requestedPackages).toContain('is-negative')
  expect(readLockfile().importers['project-2' as ProjectId].dependencies).toStrictEqual({
    'is-negative': { specifier: '1.0.0', version: '1.0.0' },
  })
})

test("a new project's plain dependency clears the optional flag it inherited", async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', optionalDependencies: { 'is-positive': '1.0.0' } },
  ])
  expect(readLockfile().snapshots['is-positive@1.0.0' as DepPath].optional).toBe(true)

  const requestedPackages = await install([
    { name: 'project-1', optionalDependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { 'is-positive': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  expect(readLockfile().snapshots['is-positive@1.0.0' as DepPath].optional).toBeUndefined()
})

test('the fast path writes the lockfile a full resolution would', async () => {
  const { install, readLockfile } = prepareWorkspace()
  const initial: WorkspaceProject[] = [
    ...TWO_LOCKED_VERSIONS,
    { name: 'project-1', optionalDependencies: { 'is-positive': '1.0.0' } },
  ]
  const extended: WorkspaceProject[] = [
    ...initial,
    {
      name: 'project-2',
      dependencies: { 'is-positive': '1.0.0' },
      devDependencies: { '@pnpm.e2e/foo': '^1.0.0' },
    },
  ]

  await install(initial)
  expect(await install(extended)).toStrictEqual([])
  const fastUpdated = readLockfile()

  await install(initial)
  await install(extended, { forceFullResolution: true })

  expect(fastUpdated).toStrictEqual(readLockfile())
})

test('a new project on a time-based lockfile writes what a full resolution would', async () => {
  const { install, readLockfile } = prepareWorkspace({ resolutionMode: 'time-based' })
  const initial: WorkspaceProject[] = [
    { name: 'project-1', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
  ]

  await install(initial)
  expect(Object.keys(readLockfile().time)).toStrictEqual(['@pnpm.e2e/pkg-with-1-dep@100.0.0'])
  // Transitive-only until now, so `time` carries no publish date for it.
  const extended: WorkspaceProject[] = [
    ...initial,
    { name: 'project-2', dependencies: { '@pnpm.e2e/dep-of-pkg-with-1-dep': lockedTransitiveVersion() } },
  ]
  expect(await install(extended)).toStrictEqual([])
  const fastUpdated = readLockfile()

  await install(initial)
  await install(extended, { forceFullResolution: true })

  expect(fastUpdated).toStrictEqual(readLockfile())

  function lockedTransitiveVersion (): string {
    const prefix = '@pnpm.e2e/dep-of-pkg-with-1-dep@'
    const depPath = Object.keys(readLockfile().packages).find((key) => key.startsWith(prefix))
    return depPath!.slice(prefix.length)
  }
})

test('a range several locked versions satisfy falls back when resolution picks the lowest', async () => {
  const { install, readLockfile } = prepareWorkspace({ resolutionMode: 'lowest-direct' })
  const extended: WorkspaceProject[] = [
    ...TWO_LOCKED_VERSIONS,
    { name: 'project-2', dependencies: { '@pnpm.e2e/foo': '^1.0.0' } },
  ]

  await install(TWO_LOCKED_VERSIONS)
  const requestedPackages = await install(extended)

  expect(requestedPackages).not.toStrictEqual([])
  expect(readLockfile().importers['project-2' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/foo': { specifier: '^1.0.0', version: '1.0.0' },
  })
})

test('a new project that depends on a workspace sibling falls back to the resolver', async () => {
  const subject = lockfileWithAnEmptyEntryForANewProject()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map([
      ['@pnpm.e2e/foo', new Map([['1.2.0', {
        rootDir: path.resolve('foo') as ProjectRootDir,
        manifest: { name: '@pnpm.e2e/foo', version: '1.2.0' },
      }]])],
    ]),
    resolutionPicksLowest: false,
    projects: [
      { id: 'project-1' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': '1.2.0' } } as ProjectManifest },
      { id: 'project-2' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': '^1.0.0' } } as ProjectManifest },
    ],
  })).toBe(false)
})

test('a new project that declares a `workspace:` dependency falls back to the resolver', async () => {
  const subject = lockfileWithAnEmptyEntryForANewProject()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [
      { id: 'project-1' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': '1.2.0' } } as ProjectManifest },
      { id: 'project-2' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': 'workspace:^' } } as ProjectManifest },
    ],
  })).toBe(false)
})

test('a new project that declares a `link:` dependency falls back to the resolver', async () => {
  const subject = lockfileWithAnEmptyEntryForANewProject()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [
      { id: 'project-1' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': '1.2.0' } } as ProjectManifest },
      { id: 'project-2' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': 'link:../foo' } } as ProjectManifest },
    ],
  })).toBe(false)
})

test('a new project that names a dependency the lockfile has never held falls back', async () => {
  const subject = lockfileWithAnEmptyEntryForANewProject()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [
      { id: 'project-1' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/foo': '1.2.0' } } as ProjectManifest },
      { id: 'project-2' as ProjectId, manifest: { dependencies: { '@pnpm.e2e/bar': '1.0.0' } } as ProjectManifest },
    ],
  })).toBe(false)
})

function lockfileWithAnEmptyEntryForANewProject (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['project-1' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '1.2.0' },
        dependencies: { '@pnpm.e2e/foo': '1.2.0' },
      },
      ['project-2' as ProjectId]: { specifiers: {} },
    },
    packages: {
      ['@pnpm.e2e/foo@1.2.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
    },
  }
}

/** Two projects between them lock two versions a `^1.0.0` range admits. */
const TWO_LOCKED_VERSIONS: WorkspaceProject[] = [
  { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '1.0.0' } },
  { name: 'project-3', dependencies: { '@pnpm.e2e/foo': '1.2.0' } },
]

interface WorkspaceProject {
  name: string
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

function prepareWorkspace (sharedOptions?: { resolutionMode: 'time-based' | 'lowest-direct' }) {
  const locations = ['project-1', 'project-2', 'project-3']
  preparePackages(locations.map((name) => ({ location: name, package: { name } })))
  const install = async (
    projects: WorkspaceProject[],
    extraOptions?: { forceFullResolution: boolean }
  ): Promise<string[]> => {
    const options = testDefaults({
      allProjects: projects.map(({ name, ...dependencyFields }) => ({
        buildIndex: 0,
        manifest: { name, version: '1.0.0', ...dependencyFields },
        rootDir: path.resolve(name) as ProjectRootDir,
      })),
      ...sharedOptions,
      ...extraOptions,
    })
    const requestedPackages = trackRequestedPackages(options.storeController)
    await mutateModules(projects.map(({ name }) => ({
      mutation: 'install' as const,
      rootDir: path.resolve(name) as ProjectRootDir,
    })), options)
    return requestedPackages
  }
  return { install, readLockfile: () => readYamlFileSync<Required<LockfileFile>>(WANTED_LOCKFILE) }
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
