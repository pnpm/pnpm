import path from 'path'
import fs from 'fs'
import { type ProjectRootDir } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { getCacheFilePath } from '../src/cacheFile'
import { type PackagesList, loadPackagesList } from '../src/index'

const lastValidatedTimestamp = Date.now()

test('loadPackagesList() when cache dir does not exist', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})

test('loadPackagesList() when cache dir exists but not the file', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath({ cacheDir, workspaceDir })
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})

test('loadPackagesList() when cache file exists but wrong schema', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath({ cacheDir, workspaceDir })
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  fs.writeFileSync(cacheFile, '"Not a valid PackagesList"')
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})

test('loadPackagesList() when cache file exists and is correct', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath({ cacheDir, workspaceDir })
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
    workspaceDir,
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual(packagesList)
})

test('loadPackagesList() when there was a hash collision', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath({ cacheDir, workspaceDir })
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  const packagesList: PackagesList = {
    lastValidatedTimestamp,
    projectRootDirs: [],
    workspaceDir: '/some/workspace/whose/path/happens/to/share/the/same/hash',
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})
