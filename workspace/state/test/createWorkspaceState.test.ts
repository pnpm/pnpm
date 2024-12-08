import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createWorkspaceState } from '../src/createWorkspaceState'

test('createWorkspaceState() on empty list', () => {
  prepareEmpty()

  expect(
    createWorkspaceState({
      allProjects: [],
      pnpmfileExists: true,
      filteredInstall: false,
      settings: {
        autoInstallPeers: true,
        dedupeDirectDeps: true,
        excludeLinksFromLockfile: false,
        preferWorkspacePackages: false,
        linkWorkspacePackages: false,
        injectWorkspacePackages: false,
      },
    })
  ).toStrictEqual(expect.objectContaining({
    projects: {},
    pnpmfileExists: true,
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
      settings: {
        autoInstallPeers: true,
        dedupeDirectDeps: true,
        excludeLinksFromLockfile: false,
        preferWorkspacePackages: false,
        linkWorkspacePackages: false,
        injectWorkspacePackages: false,
        catalogs: {
          default: {
            foo: '0.1.2',
          },
        },
      },
      pnpmfileExists: false,
      filteredInstall: false,
    })
  ).toStrictEqual(expect.objectContaining({
    settings: expect.objectContaining({
      catalogs: {
        default: {
          foo: '0.1.2',
        },
      },
    }),
    lastValidatedTimestamp: expect.any(Number),
    projects: {
      [path.resolve('packages/a')]: {},
      [path.resolve('packages/b')]: {},
      [path.resolve('packages/c')]: {},
      [path.resolve('packages/d')]: {},
    },
    pnpmfileExists: false,
  }))
})
