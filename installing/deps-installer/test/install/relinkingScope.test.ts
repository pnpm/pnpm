import fs from 'node:fs'
import path from 'node:path'

import { jest } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

const symlinkAllModulesCalls: Array<Array<{ name: string, children: string[] }>> = []

const originalWorker = await import('@pnpm/worker')
jest.unstable_mockModule('@pnpm/worker', () => ({
  ...originalWorker,
  symlinkAllModules: jest.fn(async (opts: Parameters<typeof originalWorker.symlinkAllModules>[0]) => {
    symlinkAllModulesCalls.push(
      opts.deps.map((dep) => ({
        name: dep.name,
        children: Object.keys(dep.children).sort(),
      }))
    )
    return originalWorker.symlinkAllModules(opts)
  }),
}))

const { mutateModulesInSingleProject } = await import('@pnpm/installing.deps-installer')

test('relinks only changed child edges for existing packages after dependency updates', async () => {
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  const project = prepare(manifest)

  const initialOpts = testDefaults()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, initialOpts)

  symlinkAllModulesCalls.length = 0

  const updatedOpts = testDefaults({
    overrides: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    },
    storeDir: initialOpts.storeDir,
  })
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, updatedOpts)

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('101.0.0')
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'].optionalDependencies?.['is-positive']).toBe('1.0.0')

  const pkgModulesDir = path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules')
  expect(fs.realpathSync(path.join(pkgModulesDir, '@pnpm.e2e/dep-of-pkg-with-1-dep'))).toContain('101.0.0')

  const pkgCalls = symlinkAllModulesCalls
    .flat()
    .filter((dep) => dep.name === '@pnpm.e2e/pkg-with-good-optional')

  // Existing packages with only one changed child edge should not be passed
  // through the broad worker relinking path with unchanged aliases.
  expect(pkgCalls.every(({ children }) => !children.includes('is-positive'))).toBe(true)
})

test('removes obsolete child links for existing packages after dependency updates', async () => {
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  prepare(manifest)

  const initialOpts = testDefaults()
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, initialOpts)

  const obsoleteChildPath = path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive')
  expect(fs.existsSync(obsoleteChildPath)).toBe(true)

  const updatedOpts = testDefaults({
    overrides: {
      '@pnpm.e2e/pkg-with-good-optional>is-positive': '-',
    },
    storeDir: initialOpts.storeDir,
  })
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, updatedOpts)

  expect(fs.existsSync(obsoleteChildPath)).toBe(false)
})
