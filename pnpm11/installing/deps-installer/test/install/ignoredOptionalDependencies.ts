import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import {
  testDefaults,
} from '../utils/index.js'


test('ignoredOptionalDependencies causes listed optional dependencies to be skipped', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: ['is-positive'] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['is-positive'])
  expect(lockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
})

test('empty ignoredOptionalDependencies is not recorded in lockfile', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: [] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile).not.toHaveProperty(['ignoredOptionalDependencies'])
  expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
})

test('names in ignoredOptionalDependencies are sorted alphabetically in the lockfile', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: ['foo', 'bar', 'baz', 'qux'] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['bar', 'baz', 'foo', 'qux'])
})

test('adding or changing manifest.pnpm.ignoredOptionalDependencies should change lockfile.ignoredOptionalDependencies and module structure', async () => {
  const manifest: ProjectManifest = Object.freeze({
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  })
  const project = prepareEmpty()
  const options = testDefaults()

  await install(manifest, options)
  {
    const lockfile = project.readLockfile()
    expect(lockfile).not.toHaveProperty(['ignoredOptionalDependencies'])
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  }

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.ignoredOptionalDependencies = ['is-positive']
  await install(manifest, options)
  {
    const lockfile = project.readLockfile()
    expect(requestedPackages).toStrictEqual([])
    expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['is-positive'])
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  }
})

test('removing an ignored optional dependency falls back to resolution', async () => {
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  const project = prepareEmpty()
  const options = testDefaults({
    ignoredOptionalDependencies: ['is-positive'],
  })

  await install(manifest, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.ignoredOptionalDependencies = []
  await install(manifest, options)

  expect(requestedPackages).toContain('is-positive')
  expect(project.readLockfile().packages).toHaveProperty(['is-positive@1.0.0'])
})

test('fast update prunes only optional packages that become unreachable', async () => {
  const lockfile = {
    importers: {
      '.': {
        dependencies: {
          carrier: '1.0.0',
          parent: '1.0.0',
        },
        optionalDependencies: {
          'root-only': '1.0.0',
        },
        specifiers: {
          carrier: '1.0.0',
          parent: '1.0.0',
          'root-only': '1.0.0',
        },
      },
    },
    lockfileVersion: '9.0',
    packages: {
      'carrier@1.0.0': {
        dependencies: {
          shared: '1.0.0',
        },
        resolution: { integrity: 'sha512-carrier' },
      },
      'parent@1.0.0': {
        optionalDependencies: {
          shared: '1.0.0',
          unique: '1.0.0',
        },
        resolution: { integrity: 'sha512-parent' },
      },
      'root-only@1.0.0': {
        resolution: { integrity: 'sha512-root' },
      },
      'shared@1.0.0': {
        resolution: { integrity: 'sha512-shared' },
      },
      'unique@1.0.0': {
        resolution: { integrity: 'sha512-unique' },
      },
    },
  } as LockfileObject

  expect(await tryFastUpdateIgnoredOptionalDependencies(lockfile, [
    'root-only',
    'shared',
    'unique',
  ])).toBe(true)

  const importer = lockfile.importers['.' as ProjectId]
  const parent = lockfile.packages?.['parent@1.0.0' as DepPath]
  expect(importer.optionalDependencies).toBeUndefined()
  expect(importer.specifiers).not.toHaveProperty('root-only')
  expect(parent?.optionalDependencies).toBeUndefined()
  expect(lockfile.packages).toHaveProperty(['shared@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['root-only@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['unique@1.0.0'])
})

test('fast update prunes a catalog entry its last referent was ignored', async () => {
  const lockfile = {
    catalogs: {
      default: {
        'is-positive': { specifier: '^1.0.0', version: '1.0.0' },
      },
    },
    importers: {
      '.': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': 'catalog:',
        },
      },
    },
    lockfileVersion: '9.0',
    packages: {
      'is-positive@1.0.0': { resolution: { integrity: 'sha512-pos' } },
    },
  } as unknown as LockfileObject

  expect(await tryFastUpdateIgnoredOptionalDependencies(lockfile, ['is-positive'])).toBe(true)
  expect(lockfile.catalogs).toBeUndefined()
})

test('fast update rejects a new exclusion pattern', async () => {
  const lockfile = {
    ignoredOptionalDependencies: ['*'],
    importers: {},
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(await tryFastUpdateIgnoredOptionalDependencies(lockfile, ['*', '!is-positive'])).toBe(false)
})

test('fast update rejects adding an include to exclusion-only patterns', async () => {
  const lockfile = {
    ignoredOptionalDependencies: ['!foo'],
    importers: {},
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(await tryFastUpdateIgnoredOptionalDependencies(lockfile, ['!foo', 'bar'])).toBe(false)
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
/** The composed pipeline restricted to `ignoredOptionalDependencies` drift. */
async function tryFastUpdateIgnoredOptionalDependencies (
  lockfile: LockfileObject,
  ignoredOptionalDependencies: string[]
): Promise<boolean> {
  return tryComposeFastUpdates(lockfile, {
    drift: { ignoredOptionalDependencies: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [],
    ignoredOptionalDependencies,
  })
}
