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

jest.unstable_mockModule('@pnpm/pnpr.client', () => ({ resolveViaPnprServer }))

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
