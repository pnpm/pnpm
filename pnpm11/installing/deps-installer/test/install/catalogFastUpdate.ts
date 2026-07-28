import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryFastUpdateCatalogs } from '../../src/install/tryFastUpdateCatalogs.js'
import { tryFastUpdateLockfile } from '../../src/install/tryFastUpdateLockfile.js'
import { testDefaults } from '../utils/index.js'

test('a compatible catalog range update retains the locked peer snapshot without resolution', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/has-optional-peer-with-peer': 'catalog:',
    },
  }
  const options = testDefaults({
    catalogs: {
      default: {
        '@pnpm.e2e/has-optional-peer-with-peer': '^1.0.0',
      },
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.catalogs = {
    default: {
      '@pnpm.e2e/has-optional-peer-with-peer': '>=1.0.0 <2',
    },
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  expect(project.readLockfile().catalogs.default['@pnpm.e2e/has-optional-peer-with-peer']).toStrictEqual({
    specifier: '>=1.0.0 <2',
    version: '1.0.0',
  })
})

test('an incompatible catalog range update falls back to resolution', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foobarqar': 'catalog:',
    },
  }
  const options = testDefaults({
    catalogs: {
      default: {
        '@pnpm.e2e/foobarqar': '1.0.0',
      },
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.catalogs = {
    default: {
      '@pnpm.e2e/foobarqar': '1.0.1',
    },
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toContain('@pnpm.e2e/foobarqar')
})

test('a catalog snapshot still referenced by an importer is not removed', () => {
  const lockfile = {
    catalogs: {
      default: {
        foo: {
          specifier: '^1.0.0',
          version: '1.0.0',
        },
      },
    },
    importers: {
      '.': {
        specifiers: {
          foo: 'catalog:',
        },
      },
    },
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(tryFastUpdateCatalogs(lockfile, {
    catalogs: {},
    overrides: {},
  })).toBe(false)
  expect(lockfile.catalogs).toHaveProperty(['default', 'foo'])
})

test('an unreferenced stale catalog snapshot is removed', () => {
  const lockfile = {
    catalogs: {
      default: {
        foo: {
          specifier: '^1.0.0',
          version: '1.0.0',
        },
      },
    },
    importers: {
      '.': {
        specifiers: {},
      },
    },
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(tryFastUpdateCatalogs(lockfile, {
    catalogs: {},
    overrides: {},
  })).toBe(true)
  expect(lockfile.catalogs).toBeUndefined()
})

test('a failed candidate validation leaves the lockfile unchanged', async () => {
  const lockfile = {
    catalogs: {
      default: {
        foo: {
          specifier: '^1.0.0',
          version: '1.0.0',
        },
      },
    },
    importers: {
      '.': {
        specifiers: {
          foo: 'catalog:',
        },
      },
    },
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(await tryFastUpdateLockfile(lockfile, {
    update: (candidate) => tryFastUpdateCatalogs(candidate, {
      catalogs: {
        default: {
          foo: '>=1 <2',
        },
      },
      overrides: {},
    }),
    isLockfileUpToDate: async () => false,
  })).toBe(false)
  expect(lockfile.catalogs?.default.foo.specifier).toBe('^1.0.0')
})

test('a malformed catalog version falls back without changing the snapshot', () => {
  const lockfile = {
    catalogs: {
      default: {
        foo: {
          specifier: '^1.0.0',
          version: 'not-a-version',
        },
      },
    },
    importers: {
      '.': {
        specifiers: {
          foo: 'catalog:',
        },
      },
    },
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(tryFastUpdateCatalogs(lockfile, {
    catalogs: {
      default: {
        foo: '>=1 <2',
      },
    },
    overrides: {},
  })).toBe(false)
  expect(lockfile.catalogs?.default.foo.specifier).toBe('^1.0.0')
})

function trackRequestedPackages (storeController: StoreController): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}
