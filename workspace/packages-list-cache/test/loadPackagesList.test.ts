import path from 'path'
import fs from 'fs'
import { logger } from '@pnpm/logger'
import { type ProjectRootDir } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { getCacheFilePath } from '../src/cacheFile'
import { type PackagesList, loadPackagesList } from '../src/index'

const lastValidatedTimestamp = Date.now()

const originalLoggerDebug = logger.debug
beforeEach(() => {
  logger.debug = jest.fn(originalLoggerDebug)
})
afterEach(() => {
  logger.debug = originalLoggerDebug
})

const expectedLoggerCalls = [[{ msg: 'loading packages list' }]]

test('loadPackagesList() when cache dir does not exist', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  expect(loadPackagesList(workspaceDir)).toBeUndefined()
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})

test('loadPackagesList() when cache dir exists but not the file', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath(workspaceDir)
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  expect(loadPackagesList(workspaceDir)).toBeUndefined()
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})

test('loadPackagesList() when cache file exists and is correct', async () => {
  prepareEmpty()

  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath(workspaceDir)
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  const packagesList: PackagesList = {
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    lastValidatedTimestamp,
    projectRootDirs: [
      path.resolve('packages/a') as ProjectRootDir,
      path.resolve('packages/b') as ProjectRootDir,
      path.resolve('packages/c') as ProjectRootDir,
      path.resolve('packages/d') as ProjectRootDir,
    ],
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(loadPackagesList(workspaceDir)).toStrictEqual(packagesList)
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual(expectedLoggerCalls)
})
