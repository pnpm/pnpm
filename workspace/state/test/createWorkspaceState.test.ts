import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createWorkspaceState } from '../src/createWorkspaceState'

test('createWorkspaceState() on empty list', () => {
  prepareEmpty()

  expect(
    createWorkspaceState({
      allProjects: [],
      catalogs: undefined,
      hasPnpmfile: true,
      linkWorkspacePackages: true,
      filteredInstall: false,
    })
  ).toStrictEqual(expect.objectContaining({
    catalogs: undefined,
    projects: {},
    hasPnpmfile: true,
    lastValidatedTimestamp: expect.any(Number),
  }))
})

test('createWorkspaceState() on non-empty list', () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  expect(
    createWorkspaceState({
      allProjects: [
        { rootDir: path.resolve('packages/c') as ProjectRootDir, manifest: {} },
        { rootDir: path.resolve('packages/b') as ProjectRootDir, manifest: {} },
        { rootDir: path.resolve('packages/a') as ProjectRootDir, manifest: {} },
        { rootDir: path.resolve('packages/d') as ProjectRootDir, manifest: {} },
      ],
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
      hasPnpmfile: false,
      linkWorkspacePackages: true,
      filteredInstall: false,
    })
  ).toStrictEqual(expect.objectContaining({
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    lastValidatedTimestamp: expect.any(Number),
    projects: {
      [path.resolve('packages/a')]: {},
      [path.resolve('packages/b')]: {},
      [path.resolve('packages/c')]: {},
      [path.resolve('packages/d')]: {},
    },
    hasPnpmfile: false,
  }))
})
