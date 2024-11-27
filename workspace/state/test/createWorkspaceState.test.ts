import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createWorkspaceState } from '../src/createWorkspaceState'

const lastValidatedTimestamp = Date.now()

test('createWorkspaceState() on empty list', () => {
  prepareEmpty()

  expect(
    createWorkspaceState({
      allProjects: [],
      catalogs: undefined,
      lastValidatedTimestamp,
    })
  ).toStrictEqual({
    catalogs: undefined,
    lastValidatedTimestamp,
    projectRootDirs: [],
  })
})

test('createWorkspaceState() on non-empty list', () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  expect(
    createWorkspaceState({
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
  })
})
