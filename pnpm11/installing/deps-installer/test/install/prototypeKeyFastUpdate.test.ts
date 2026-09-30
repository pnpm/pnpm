import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

import type { GraphEdits } from '../../src/install/tryComposeFastUpdates.js'
import { pruneUnreferencedCatalogEntries } from '../../src/install/tryFastUpdateCatalogs.js'
import { tryFastUpdateCatalogVersions } from '../../src/install/tryFastUpdateCatalogVersions.js'
import { tryFastUpdateImporters } from '../../src/install/tryFastUpdateImporters.js'

// `constructor` is a valid package name that is also an `Object.prototype`
// member, so a plain-object lookup of it never comes back empty.

function lockfileWithImporter (importer: LockfileObject['importers'][ProjectId]): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { ['.' as ProjectId]: importer },
    packages: {
      ['constructor@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-AAAA' } },
      ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-BBBB' } },
    },
  } as LockfileObject
}

function updateImporters (lockfile: LockfileObject, manifest: ProjectManifest): { changed: boolean, edits: GraphEdits } {
  const edits: GraphEdits = { dropped: new Set(), optionalFlagsAreStale: false }
  const changed = tryFastUpdateImporters(lockfile, {
    projects: [{ id: '.' as ProjectId, manifest }],
    pruneLockfileImporters: false,
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
  }, edits)
  return { changed, edits }
}

test('a dependency named constructor is added to an importer from the locked version', () => {
  const lockfile = lockfileWithImporter({
    dependencies: { foo: '1.0.0' },
    specifiers: { foo: '^1.0.0' },
  })
  const { changed } = updateImporters(lockfile, {
    dependencies: { foo: '^1.0.0', constructor: '^1.0.0' },
    optionalDependencies: { foo: '^1.0.0' },
  })
  expect(changed).toBe(true)
  const importer = lockfile.importers['.' as ProjectId]
  expect(Object.hasOwn(importer.dependencies!, 'constructor')).toBe(true)
  expect(importer.dependencies!.constructor).toBe('1.0.0')
  expect(importer.specifiers.constructor).toBe('^1.0.0')
})

test('a dependency named constructor is dropped from an importer once the manifest removes it', () => {
  const lockfile = lockfileWithImporter({
    dependencies: { foo: '1.0.0', constructor: '1.0.0' },
    specifiers: { foo: '^1.0.0', constructor: '^1.0.0' },
  })
  const { changed, edits } = updateImporters(lockfile, {
    dependencies: { foo: '^1.0.0' },
  })
  expect(changed).toBe(true)
  const importer = lockfile.importers['.' as ProjectId]
  expect(Object.hasOwn(importer.dependencies!, 'constructor')).toBe(false)
  expect(Object.hasOwn(importer.specifiers, 'constructor')).toBe(false)
  expect(edits.dropped.size).toBe(1)
})

test('pruneUnreferencedCatalogEntries() drops a catalog entry named constructor that no importer references', () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    catalogs: {
      default: {
        constructor: { specifier: '^1.0.0', version: '1.0.0' },
      },
    },
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  } as LockfileObject
  pruneUnreferencedCatalogEntries(lockfile)
  expect(lockfile.catalogs).toBeUndefined()
})

test('tryFastUpdateCatalogVersions() checks a catalog entry named constructor against importers that do not depend on it', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    catalogs: {
      default: {
        constructor: { specifier: '^1.0.0', version: '1.0.0' },
      },
    },
    importers: {
      ['.' as ProjectId]: {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
      ['b' as ProjectId]: {
        dependencies: { constructor: '1.0.0' },
        specifiers: { constructor: 'catalog:' },
      },
    },
  } as LockfileObject
  // The override hands the move to the resolver once the sole-reference check
  // has passed, so no registry request is needed.
  const result = await tryFastUpdateCatalogVersions(lockfile, {
    catalogs: { default: { constructor: '2.0.0' } },
    parsedOverrides: [{ selector: 'constructor', newBareSpecifier: '2.0.0', targetPkg: { name: 'constructor' } }],
  } as unknown as Parameters<typeof tryFastUpdateCatalogVersions>[1])
  expect(result).toBe('unsupported')
})
