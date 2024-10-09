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
    modificationTimestamps: {},
    workspaceDir,
  })

  await updatePackagesList({
    cacheDir,
    workspaceDir,
    allProjects: [
      { rootDir: path.resolve('packages/c') as ProjectRootDir },
      { rootDir: path.resolve('packages/a') as ProjectRootDir },
      { rootDir: path.resolve('packages/d') as ProjectRootDir },
      { rootDir: path.resolve('packages/b') as ProjectRootDir },
    ],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    modificationTimestamps: {
      [path.resolve('packages/a')]: {
        'package.json': timeTables.a,
      },
      [path.resolve('packages/b')]: {
        'package.json': timeTables.b,
      },
      [path.resolve('packages/c')]: {
        'package.json': timeTables.c,
      },
      [path.resolve('packages/d')]: {
        'package.json': timeTables.d,
      },
    },
    workspaceDir,
  })
})
