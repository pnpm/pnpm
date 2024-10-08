import path from 'path'
import fs from 'fs'
import { type ProjectRootDir } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { getCacheFilePath } from '../src/cacheFile'
import { type PackagesList, loadPackagesList } from '../src/index'

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
    projectRootDirs: [
      '/home/user/repos/my-project/packages/a' as ProjectRootDir,
      '/home/user/repos/my-project/packages/b' as ProjectRootDir,
      '/home/user/repos/my-project/packages/c' as ProjectRootDir,
      '/home/user/repos/my-project/packages/d' as ProjectRootDir,
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
    projectRootDirs: [
      '/home/user/repos/my-project/packages/a' as ProjectRootDir,
      '/home/user/repos/my-project/packages/b' as ProjectRootDir,
      '/home/user/repos/my-project/packages/c' as ProjectRootDir,
      '/home/user/repos/my-project/packages/d' as ProjectRootDir,
    ],
    workspaceDir: '/some/workspace/whose/path/happens/to/share/the/same/hash',
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})
