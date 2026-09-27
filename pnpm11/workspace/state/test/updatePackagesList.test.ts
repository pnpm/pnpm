import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { logger } from '@pnpm/logger'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { getFilePath } from '../src/filePath.js'
import { loadWorkspaceState, updateWorkspaceState } from '../src/index.js'

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
    pnpmfiles: [],
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
  expect(jest.mocked(logger.debug).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual(expect.objectContaining({
    lastValidatedTimestamp: expect.any(Number),
    projects: {},
  }))

  logger.debug = jest.fn(originalLoggerDebug)
  await updateWorkspaceState({
    pnpmfiles: [],
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
  expect(jest.mocked(logger.debug).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
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

test('updateWorkspaceState() does not throw when cache file writing fails', async () => {
  preparePackages([])
  const workspaceDir = process.cwd()
  const cacheFile = getFilePath(workspaceDir)
  await fs.promises.mkdir(cacheFile, { recursive: true })

  await expect(updateWorkspaceState({
    pnpmfiles: [],
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
  })).resolves.toBeUndefined()
})

test('updateWorkspaceState() records which hoisted projects have their own modules directory', async () => {
  preparePackages([
    { location: './packages/nested', package: { name: 'nested' } },
    { location: './packages/flat', package: { name: 'flat' } },
  ])
  fs.mkdirSync('packages/nested/node_modules')
  const workspaceDir = process.cwd()
  const allProjects = [
    { rootDir: path.resolve('packages/nested') as ProjectRootDir, manifest: { name: 'nested' } },
    { rootDir: path.resolve('packages/flat') as ProjectRootDir, manifest: { name: 'flat' } },
  ]
  const settings = {
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    preferWorkspacePackages: false,
  }

  await updateWorkspaceState({
    pnpmfiles: [],
    workspaceDir,
    allProjects,
    filteredInstall: false,
    settings: { ...settings, nodeLinker: 'hoisted' },
  })
  expect(loadWorkspaceState(workspaceDir)?.projects).toStrictEqual({
    [path.resolve('packages/nested')]: { name: 'nested', hasModulesDir: true },
    [path.resolve('packages/flat')]: { name: 'flat' },
  })

  await updateWorkspaceState({
    pnpmfiles: [],
    workspaceDir,
    allProjects,
    filteredInstall: false,
    settings: { ...settings, nodeLinker: 'isolated' },
  })
  expect(loadWorkspaceState(workspaceDir)?.projects).toStrictEqual({
    [path.resolve('packages/nested')]: { name: 'nested' },
    [path.resolve('packages/flat')]: { name: 'flat' },
  })
})
