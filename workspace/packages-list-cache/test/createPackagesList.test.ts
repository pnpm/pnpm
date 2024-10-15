import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createPackagesList } from '../src/createPackagesList'

const lastValidatedTimestamp = Date.now()

test('createPackagesList() on empty list', () => {
  prepareEmpty()

  const workspaceDir = process.cwd()

  expect(
    createPackagesList({
      allProjects: [],
      lastValidatedTimestamp,
      workspaceDir,
    })
  ).toStrictEqual({
    catalogs: undefined,
    lastValidatedTimestamp,
    projectRootDirs: [],
    workspaceDir,
  })
})

test('createPackagesList() on non-empty list', () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const workspaceDir = process.cwd()

  expect(
    createPackagesList({
      allProjects: [
        { rootDir: path.resolve('packages/c') as ProjectRootDir },
        { rootDir: path.resolve('packages/b') as ProjectRootDir },
        { rootDir: path.resolve('packages/a') as ProjectRootDir },
        { rootDir: path.resolve('packages/d') as ProjectRootDir },
      ],
      lastValidatedTimestamp,
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
      workspaceDir,
    })
  ).toStrictEqual({
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    lastValidatedTimestamp,
    projectRootDirs: [
      path.resolve('packages/a'),
      path.resolve('packages/b'),
      path.resolve('packages/c'),
      path.resolve('packages/d'),
    ],
    workspaceDir,
  })
})
