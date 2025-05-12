import path from 'path'
import { logger } from '@pnpm/logger'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadWorkspaceState, updateWorkspaceState } from '../src/index'

const originalLoggerDebug = logger.debug
afterEach(() => {
  logger.debug = originalLoggerDebug
})

test('updateWorkspaceState()', async () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const workspaceDir = process.cwd()

  expect(loadWorkspaceState(workspaceDir)).toBeUndefined()

  logger.debug = jest.fn(originalLoggerDebug)
  await updateWorkspaceState({
    pnpmfileExists: true,
    workspaceDir,
    allProjects: [],
    filteredInstall: false,
    settings: {
      autoInstallPeers: true,
      dedupeDirectDeps: true,
      excludeLinksFromLockfile: false,
      preferWorkspacePackages: false,
      linkWorkspacePackages: false,
      injectWorkspacePackages: false,
    },
  })
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual(expect.objectContaining({
    lastValidatedTimestamp: expect.any(Number),
    projects: {},
  }))

  logger.debug = jest.fn(originalLoggerDebug)
  await updateWorkspaceState({
    pnpmfileExists: false,
    workspaceDir,
    settings: {
      autoInstallPeers: true,
      dedupeDirectDeps: true,
      excludeLinksFromLockfile: false,
      preferWorkspacePackages: false,
      injectWorkspacePackages: false,
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
      linkWorkspacePackages: true,
    },
    allProjects: [
      { rootDir: path.resolve('packages/c') as ProjectRootDir, manifest: {} },
      { rootDir: path.resolve('packages/a') as ProjectRootDir, manifest: {} },
      { rootDir: path.resolve('packages/d') as ProjectRootDir, manifest: {} },
      { rootDir: path.resolve('packages/b') as ProjectRootDir, manifest: {} },
    ],
    filteredInstall: false,
  })
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual(expect.objectContaining({
    settings: expect.objectContaining({
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
    }),
    lastValidatedTimestamp: expect.any(Number),
    projects: {
      [path.resolve('packages/a')]: {},
      [path.resolve('packages/b')]: {},
      [path.resolve('packages/c')]: {},
      [path.resolve('packages/d')]: {},
    },
  }))
})
