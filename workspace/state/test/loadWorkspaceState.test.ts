import path from 'path'
import fs from 'fs'
import { logger } from '@pnpm/logger'
import { type ProjectRootDir } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { getFilePath } from '../src/filePath'
import { type WorkspaceState, loadWorkspaceState } from '../src/index'

const lastValidatedTimestamp = Date.now()

const originalLoggerDebug = logger.debug
beforeEach(() => {
  logger.debug = jest.fn(originalLoggerDebug)
})
afterEach(() => {
  logger.debug = originalLoggerDebug
})

const expectedLoggerCalls = [[{ msg: 'loading workspace state' }]]

test('loadWorkspaceState() when cache dir does not exist', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  expect(loadWorkspaceState(workspaceDir)).toBeUndefined()
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})

test('loadWorkspaceState() when cache dir exists but not the file', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const cacheFile = getFilePath(workspaceDir)
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  expect(loadWorkspaceState(workspaceDir)).toBeUndefined()
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})

test('loadWorkspaceState() when cache file exists and is correct', async () => {
  prepareEmpty()

  const workspaceDir = process.cwd()
  const cacheFile = getFilePath(workspaceDir)
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  const workspaceState: WorkspaceState = {
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
    lastValidatedTimestamp,
    projects: {
      [path.resolve('packages/a') as ProjectRootDir]: {},
      [path.resolve('packages/b') as ProjectRootDir]: {},
      [path.resolve('packages/c') as ProjectRootDir]: {},
      [path.resolve('packages/d') as ProjectRootDir]: {},
    },
    pnpmfileExists: false,
    filteredInstall: false,
  }
  fs.writeFileSync(cacheFile, JSON.stringify(workspaceState))
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual(workspaceState)
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})
