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

  const timeTables = {
    a: 1_728_400_000_000,
    b: 1_728_500_000_000,
    c: 1_728_450_000_000,
    d: 1_728_600_000_000,
  }

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
    projects: {
      [path.resolve('packages/a') as ProjectRootDir]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.a,
      },
      [path.resolve('packages/b') as ProjectRootDir]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.b,
      },
      [path.resolve('packages/c') as ProjectRootDir]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.c,
      },
      [path.resolve('packages/d') as ProjectRootDir]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.d,
      },
    },
    workspaceDir,
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual(packagesList)
})

test.todo('loadPackagesList() with package.json, package.json5, package.yaml')

test('loadPackagesList() when there was a hash collision', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()
  const cacheFile = getCacheFilePath({ cacheDir, workspaceDir })
  fs.mkdirSync(path.dirname(cacheFile), { recursive: true })
  const packagesList: PackagesList = {
    projects: {},
    workspaceDir: '/some/workspace/whose/path/happens/to/share/the/same/hash',
  }
  fs.writeFileSync(cacheFile, JSON.stringify(packagesList))
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()
})
