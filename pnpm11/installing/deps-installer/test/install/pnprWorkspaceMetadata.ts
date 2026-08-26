import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import type { MutateModulesOptions, ProjectOptions } from '@pnpm/installing.deps-installer'
import type { ResolveViaPnprServerOptions, ResolveViaPnprServerResult } from '@pnpm/pnpr.client'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

const originalCwd = process.cwd()
const storeControllers: StoreController[] = []
const pnprResolutionSettings: Array<[
  string,
  Partial<MutateModulesOptions>,
  Record<string, unknown>
]> = [
  ['patchedDependencies', {
    allowUnusedPatches: true,
    patchedDependencies: {
      'unused@1.0.0': path.join(import.meta.dirname, '../fixtures/patch-pkg/is-positive@1.0.0.patch'),
    },
  }, {
    allowUnusedPatches: true,
    patchedDependencies: { 'unused@1.0.0': expect.any(String) },
  }],
  ['packageExtensions', {
    packageExtensions: {
      'unused@1.0.0': { dependencies: { 'is-positive': '1.0.0' } },
    },
  }, {
    packageExtensions: {
      'unused@1.0.0': { dependencies: { 'is-positive': '1.0.0' } },
    },
  }],
]
const resolveViaPnprServer = jest.fn(async (
  options: ResolveViaPnprServerOptions
): Promise<ResolveViaPnprServerResult> => {
  const importerDirs = options.projects?.map(({ dir }) => dir) ?? ['.']
  return {
    lockfile: {
      lockfileVersion: '9.0',
      importers: Object.fromEntries(importerDirs.map((dir) => [dir, { specifiers: {} }])),
      packages: {},
    },
    stats: { totalPackages: 0 },
  }
})

jest.unstable_mockModule('@pnpm/pnpr.client', () => ({
  canRestoreRemoteSideEffects: () => false,
  createRemoteSideEffectsRestorer: () => undefined,
  publishBuiltSharedSideEffects: async () => undefined,
  resolveViaPnprServer,
}))

const { install, mutateModules } = await import('@pnpm/installing.deps-installer')

afterEach(async () => {
  try {
    await Promise.all(storeControllers.splice(0).map(async (storeController) => storeController.close()))
    resolveViaPnprServer.mockClear()
  } finally {
    process.chdir(originalCwd)
  }
})

test("pnpr forwards a single project's name and version", async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir)

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledTimes(1)
  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    name: 'app',
    version: '1.2.3',
    dependencies: undefined,
    devDependencies: undefined,
    optionalDependencies: undefined,
    projects: undefined,
  }))
})

test('pnpr forwards catalogs and overrides so the server can resolve catalog references', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    catalogs: { default: { '@tanstack/store': '0.11.0' } },
    overrides: { '@tanstack/store': 'catalog:', foo: '1.0.0' },
  })

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledTimes(1)
  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    catalogs: { default: { '@tanstack/store': '0.11.0' } },
    overrides: { '@tanstack/store': 'catalog:', foo: '1.0.0' },
  }))
})

test("pnpr forwards every workspace project's name and version", async () => {
  const workspaceRoot = prepareEmpty().dir()
  const appManifest: ProjectManifest = {
    name: 'app',
    version: '1.0.0',
    dependencies: { lib: 'workspace:*' },
  }
  const libManifest: ProjectManifest = { name: 'lib', version: '2.0.0' }
  preparePackages([
    { location: 'packages/app', package: appManifest },
    { location: 'packages/lib', package: libManifest },
  ], { tempDir: path.join(workspaceRoot, '.fixture-anchor') })

  const appRootDir = path.join(workspaceRoot, 'packages/app') as ProjectRootDir
  const libRootDir = path.join(workspaceRoot, 'packages/lib') as ProjectRootDir
  const allProjects = [
    { buildIndex: 0, manifest: appManifest, rootDir: appRootDir },
    { buildIndex: 0, manifest: libManifest, rootDir: libRootDir },
  ] satisfies ProjectOptions[]
  const options = createOptions(workspaceRoot, appRootDir, { allProjects })

  await mutateModules([
    { mutation: 'install', rootDir: appRootDir },
    { mutation: 'install', rootDir: libRootDir },
  ], options)

  expect(resolveViaPnprServer).toHaveBeenCalledTimes(1)
  const projects = resolveViaPnprServer.mock.calls[0][0].projects
  expect(projects).toStrictEqual([
    {
      dir: 'packages/app',
      name: 'app',
      version: '1.0.0',
      dependencies: { lib: 'workspace:*' },
      devDependencies: undefined,
      optionalDependencies: undefined,
    },
    {
      dir: 'packages/lib',
      name: 'lib',
      version: '2.0.0',
      dependencies: undefined,
      devDependencies: undefined,
      optionalDependencies: undefined,
    },
  ])
  for (const project of projects ?? []) {
    expect(path.isAbsolute(project.dir)).toBe(false)
    expect(project.dir).not.toContain('\\')
  }
})

test('pnpr returns the resolution policy violations the install command reacts to', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    allProjects: [{ buildIndex: 0, manifest, rootDir }],
  })

  const result = await mutateModules([{ mutation: 'install', rootDir }], options)

  expect(result.resolutionPolicyViolations).toStrictEqual([])
})

test("pnpr forwards the client's whole verification policy, not just the age cutoff", async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    minimumReleaseAge: 1440,
    minimumReleaseAgeExclude: ['@acme/*'],
    minimumReleaseAgeIgnoreMissingTime: false,
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['legacy-pkg'],
    trustPolicyIgnoreAfter: 43200,
    trustLockfile: true,
  })

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    minimumReleaseAge: 1440,
    minimumReleaseAgeExclude: ['@acme/*'],
    minimumReleaseAgeIgnoreMissingTime: false,
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['legacy-pkg'],
    trustPolicyIgnoreAfter: 43200,
    trustLockfile: true,
  }))
})

test("pnpr forwards the client's current resolver settings, not just the last install's", async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const resolverSettings = {
    autoInstallPeers: false,
    dedupePeers: true,
    excludeLinksFromLockfile: true,
  } satisfies Partial<MutateModulesOptions>
  const options = createOptions(workspaceRoot, rootDir, resolverSettings)

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining(resolverSettings))
})

test('pnpr forwards the resolution mode so --frozen-lockfile is not silently ignored', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    frozenLockfile: true,
    preferFrozenLockfile: false,
  })

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    frozenLockfile: true,
    preferFrozenLockfile: false,
  }))
})

test('pnpr runs under trustPolicy instead of refusing the install', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, { trustPolicy: 'no-downgrade' })

  await expect(install(manifest, options)).resolves.toBeDefined()

  expect(resolveViaPnprServer).toHaveBeenCalledTimes(1)
})

test('updatePatches is delegated to pnpr', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir)

  await install(manifest, {
    ...options,
    depth: Infinity,
    update: true,
    updatePatches: true,
  })

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    updatePatches: true,
  }))
})

test('a complete updatePatches mutation is delegated to pnpr', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    allProjects: [{ buildIndex: 0, manifest, rootDir }],
  })

  await mutateModules([{ mutation: 'install', rootDir, updatePatches: true }], options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    updatePatches: true,
  }))
})

test('an updatePatches install with a depth limit stays on the client', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir)

  await install(manifest, { ...options, depth: 0, update: true, updatePatches: true })

  expect(resolveViaPnprServer).not.toHaveBeenCalled()
})

test('an updatePatches mutation with filtered dependency groups stays on the client', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    allProjects: [{ buildIndex: 0, manifest, rootDir }],
    depth: Infinity,
    includeDirect: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
  })

  await mutateModules([{ mutation: 'install', rootDir, update: true, updatePatches: true }], options)

  expect(resolveViaPnprServer).not.toHaveBeenCalled()
})

test('unsupported direct update modes stay on the client', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir)

  await install(manifest, { ...options, update: true })

  expect(resolveViaPnprServer).not.toHaveBeenCalled()
})

test('a partial updatePatches mutation stays on the client', async () => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const otherRootDir = path.join(workspaceRoot, 'other') as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    allProjects: [
      { buildIndex: 0, manifest, rootDir },
      { buildIndex: 1, manifest: { name: 'other', version: '1.0.0' }, rootDir: otherRootDir },
    ],
  })

  await mutateModules([{ mutation: 'install', rootDir, updatePatches: true }], options)

  expect(resolveViaPnprServer).not.toHaveBeenCalled()
})

test.each(pnprResolutionSettings)('install forwards %s to pnpr', async (_settingName, settings, expected) => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, settings)

  await install(manifest, options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining(expected))
})

test.each(pnprResolutionSettings)('a mutation forwards %s to pnpr', async (_settingName, settings, expected) => {
  const workspaceRoot = prepareEmpty().dir()
  const rootDir = workspaceRoot as ProjectRootDir
  const manifest: ProjectManifest = { name: 'app', version: '1.2.3' }
  const options = createOptions(workspaceRoot, rootDir, {
    ...settings,
    allProjects: [{ buildIndex: 0, manifest, rootDir }],
  })

  await mutateModules([{ mutation: 'install', rootDir }], options)

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining(expected))
})

function createOptions (
  workspaceRoot: string,
  rootDir: ProjectRootDir,
  overrides: Partial<MutateModulesOptions> = {}
): MutateModulesOptions {
  const options = testDefaults({
    pnprServer: 'http://pnpr.test',
    lockfileOnly: true,
    dir: rootDir,
    lockfileDir: workspaceRoot,
    storeDir: path.join(workspaceRoot, '.store'),
    cacheDir: path.join(workspaceRoot, '.cache'),
    ...overrides,
  })
  storeControllers.push(options.storeController)
  return options
}
