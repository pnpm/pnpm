import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
import { tryFastUpdateCatalogs } from '../../src/install/tryFastUpdateCatalogs.js'
import type { Project } from '../../src/install/tryFastUpdateImporters.js'
import { tryFastUpdateLockfile } from '../../src/install/tryFastUpdateLockfile.js'
import { testDefaults } from '../utils/index.js'


test('a compatible package range update retains the locked peer snapshot without resolution', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/has-optional-peer-with-peer': '^1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  manifest.dependencies!['@pnpm.e2e/has-optional-peer-with-peer'] = '>=1.0.0 <2'

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  expect(project.readLockfile().importers['.' as ProjectId].dependencies!['@pnpm.e2e/has-optional-peer-with-peer']).toStrictEqual({
    specifier: '>=1.0.0 <2',
    version: '1.0.0',
  })
})

test('an incompatible package range update falls back without mutating the lockfile', async () => {
  const lockfile = {
    importers: {
      '.': {
        dependencies: {
          bar: '1.0.0',
          foo: '1.0.0',
        },
        specifiers: {
          bar: '^1.0.0',
          foo: '^1.0.0',
        },
      },
    },
    lockfileVersion: '9.0',
  } as LockfileObject

  expect(await tryFastUpdateLockfile(lockfile, {
    update: async (candidate) => tryFastUpdateImporters(candidate, [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          foo: '>=1.0.0 <2',
          bar: '^2.0.0',
        },
      },
    }]),
    isLockfileUpToDate: async () => true,
  })).toBe(false)
  expect(lockfile.importers['.' as ProjectId].specifiers).toStrictEqual({
    bar: '^1.0.0',
    foo: '^1.0.0',
  })
})

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

test('simultaneous catalog and override changes are absorbed in one pass', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/parent-of-pkg-with-1-dep': 'catalog:',
    },
  }
  const options = testDefaults({
    catalogs: {
      default: {
        '@pnpm.e2e/parent-of-pkg-with-1-dep': '^1.0.0',
      },
    },
    overrides: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
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
      '@pnpm.e2e/parent-of-pkg-with-1-dep': '>=1 <2',
    },
  }
  options.overrides = {
    '@pnpm.e2e/pkg-with-1-dep': '100.1.0',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  // Only the version the override names is resolved; the catalog entry moves
  // through the range-only rewrite on the same candidate.
  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/pkg-with-1-dep'])
  const written = project.readLockfile()
  expect(written.catalogs.default['@pnpm.e2e/parent-of-pkg-with-1-dep'].specifier).toBe('>=1 <2')
  expect(written.snapshots['@pnpm.e2e/parent-of-pkg-with-1-dep@1.0.0'].dependencies?.['@pnpm.e2e/pkg-with-1-dep'])
    .toBe('100.1.0')
})

test('a catalog edit and a removal override are absorbed in one pass', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': 'catalog:',
    },
  }
  const options = testDefaults({
    catalogs: {
      default: {
        '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
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
      '@pnpm.e2e/pkg-with-good-optional': '>=1.0.0 <2',
    },
  }
  options.overrides = {
    'is-positive': '-',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  // A removal names no new version, so the composed pass introduces nothing
  // the registry has to answer for.
  expect(requestedPackages).toStrictEqual([])
  const written = project.readLockfile()
  expect(written.catalogs.default['@pnpm.e2e/pkg-with-good-optional']).toStrictEqual({
    specifier: '>=1.0.0 <2',
    version: '1.0.0',
  })
  expect(written.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'].optionalDependencies)
    .toBeUndefined()
  for (const section of [written.snapshots, written.packages]) {
    expect(Object.keys(section).some((depPath) => depPath.startsWith('is-positive@'))).toBe(false)
  }
})

test.each(['catalog:', 'catalog:default'])('a default catalog snapshot referenced as %s is not removed', (specifier) => {
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
          foo: specifier,
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

test('a referenced catalog entry missing from the snapshots falls back', () => {
  const lockfile = {
    catalogs: {
      default: {
        bar: {
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
    catalogs: {
      default: {
        foo: '^1.0.0',
      },
    },
    overrides: {},
  })).toBe(false)
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
/** The composed pipeline restricted to manifest drift. */
async function tryFastUpdateImporters (lockfile: LockfileObject, projects: Project[]): Promise<boolean> {
  return tryComposeFastUpdates(lockfile, { drift: { importers: true }, projects, workspacePackages: new Map(), resolutionPicksLowest: false })
}
