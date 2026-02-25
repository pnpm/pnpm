import path from 'path'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { createWorkspaceState } from '../src/createWorkspaceState.js'

test('createWorkspaceState() on empty list', () => {
  prepareEmpty()

  expect(
    createWorkspaceState({
      allProjects: [],
      pnpmfiles: [],
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
    pnpmfiles: [],
    lastValidatedTimestamp: expect.any(Number),
  }))
})

test('createWorkspaceState() saves lockfile-affecting settings', () => {
  prepareEmpty()

  const state = createWorkspaceState({
    allProjects: [],
    pnpmfiles: [],
    filteredInstall: false,
    settings: {
      autoInstallPeers: true,
      dedupeDirectDeps: true,
      excludeLinksFromLockfile: false,
      preferWorkspacePackages: false,
      linkWorkspacePackages: false,
      injectWorkspacePackages: false,
      overrides: {
        foo: '1.0.0',
      },
      packageExtensions: {
        bar: { dependencies: { baz: '2.0.0' } },
      },
      ignoredOptionalDependencies: ['qux'],
      patchedDependencies: {
        'some-pkg': 'patches/some-pkg.patch',
      },
      peersSuffixMaxLength: 100,
    },
  })

  expect(state.settings.overrides).toStrictEqual({ foo: '1.0.0' })
  expect(state.settings.packageExtensions).toStrictEqual({
    bar: { dependencies: { baz: '2.0.0' } },
  })
  expect(state.settings.ignoredOptionalDependencies).toStrictEqual(['qux'])
  expect(state.settings.patchedDependencies).toStrictEqual({
    'some-pkg': 'patches/some-pkg.patch',
  })
  expect(state.settings.peersSuffixMaxLength).toBe(100)
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
      pnpmfiles: [],
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
    pnpmfiles: [],
  }))
})
