import fs from 'fs'
import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createPackagesList } from '../src/createPackagesList'

test('createPackagesList() on empty list', async () => {
  prepareEmpty()

  const workspaceDir = process.cwd()

  expect(
    await createPackagesList({
      allProjects: [],
      workspaceDir,
    })
  ).toStrictEqual({
    catalogs: undefined,
    projects: {},
    workspaceDir,
  })
})

test('createPackagesList() on non-empty list', async () => {
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

  const workspaceDir = process.cwd()

  expect(
    await createPackagesList({
      allProjects: [
        { rootDir: path.resolve('packages/a') as ProjectRootDir },
        { rootDir: path.resolve('packages/b') as ProjectRootDir },
        { rootDir: path.resolve('packages/c') as ProjectRootDir },
        { rootDir: path.resolve('packages/d') as ProjectRootDir },
      ],
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
      workspaceDir,
    })
  ).toStrictEqual({
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
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    workspaceDir,
  })
})

test.todo('createPackagesList() with package.json, package.json5, package.yaml')
