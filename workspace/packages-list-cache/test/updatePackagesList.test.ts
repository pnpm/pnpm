import path from 'path'
import fs from 'fs'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadPackagesList, updatePackagesList } from '../src/index'

test('updatePackagesList()', async () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const timeTables = {
    a: 1_728_400_000_000,
    b: 1_728_500_000_000,
    c: 1_728_450_000_000,
    d: 1_728_600_000_000,
  }
  for (const [name, timestamp] of Object.entries(timeTables)) {
    const manifestPath = path.resolve('packages', name, 'package.json')
    const date = new Date(timestamp)
    fs.utimesSync(manifestPath, date, date)
  }

  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()

  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()

  await updatePackagesList({
    cacheDir,
    workspaceDir,
    allProjects: [],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    projects: {},
    workspaceDir,
  })

  await updatePackagesList({
    cacheDir,
    workspaceDir,
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    allProjects: [
      { rootDir: path.resolve('packages/c') as ProjectRootDir },
      { rootDir: path.resolve('packages/a') as ProjectRootDir },
      { rootDir: path.resolve('packages/d') as ProjectRootDir },
      { rootDir: path.resolve('packages/b') as ProjectRootDir },
    ],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    projects: {
      [path.resolve('packages/a')]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.a,
      },
      [path.resolve('packages/b')]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.b,
      },
      [path.resolve('packages/c')]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.c,
      },
      [path.resolve('packages/d')]: {
        manifestBaseName: 'package.json',
        manifestModificationTimestamp: timeTables.d,
      },
    },
    workspaceDir,
  })

  // TODO: change the modification times and test

  // TODO: add a test with package.json5 and package.yaml
})
