import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'

import { tryFastUpdateCatalogVersions } from '../../src/install/tryFastUpdateCatalogVersions.js'

/** `target` is a direct dependency of the importer, reached only through the default catalog. */
function lockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    catalogs: {
      default: { target: { specifier: '1.0.0', version: '1.0.0' } },
    },
    importers: {
      ['.' as ProjectId]: {
        specifiers: { target: 'catalog:' },
        dependencies: { target: '1.0.0' },
      },
    },
    packages: {
      ['target@1.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-target-1' },
        dependencies: { child: '1.1.0' },
      },
      ['child@1.1.0' as DepPath]: { resolution: { integrity: 'sha512-child' } },
    },
  }
}

function updateOptions (childRange: string, catalogSpecifier = '2.0.0') {
  return {
    catalogs: { default: { target: catalogSpecifier } },
    parsedOverrides: [],
    lockfileDir: '/project',
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    requestPackage: async () => ({
      body: {
        id: 'target@2.0.0',
        resolvedVia: 'npm-registry',
        resolution: {
          integrity: 'sha512-target-2',
          tarball: 'https://registry.npmjs.org/target/-/target-2.0.0.tgz',
        },
        manifest: { name: 'target', version: '2.0.0', dependencies: { child: childRange } },
      },
    }),
    isLockfileUpToDate: async () => true,
  } as unknown as Parameters<typeof tryFastUpdateCatalogVersions>[1]
}

test('a catalog entry moved to a new exact version replaces the package', async () => {
  const subject = lockfile()

  expect(await tryFastUpdateCatalogVersions(subject, updateOptions('^1.0.0'))).toBe('applied')
  expect(Object.keys(subject.packages!).sort()).toStrictEqual(['child@1.1.0', 'target@2.0.0'])
  expect(subject.catalogs!.default.target).toStrictEqual({ specifier: '2.0.0', version: '2.0.0' })
  expect(subject.importers['.' as ProjectId].dependencies).toStrictEqual({ target: '2.0.0' })
})

test('a locked child the new version no longer admits falls back', async () => {
  expect(await tryFastUpdateCatalogVersions(lockfile(), updateOptions('^2.0.0'))).toBe('unsupported')
})

test('an importer depending on the package directly falls back', async () => {
  const subject = lockfile()
  subject.importers['pkg-a' as ProjectId] = {
    specifiers: { target: '1.0.0' },
    dependencies: { target: '1.0.0' },
  }

  expect(await tryFastUpdateCatalogVersions(subject, updateOptions('^1.0.0'))).toBe('unsupported')
})

test('a package depending on the catalog package falls back', async () => {
  const subject = lockfile()
  subject.packages!['parent@1.0.0' as DepPath] = {
    resolution: { integrity: 'sha512-parent' },
    dependencies: { target: '1.0.0' },
  }

  expect(await tryFastUpdateCatalogVersions(subject, updateOptions('^1.0.0'))).toBe('unsupported')
})

test('a catalog reference with no recorded entry falls back', async () => {
  const subject = lockfile()
  subject.importers['pkg-a' as ProjectId] = {
    specifiers: { other: 'catalog:' },
    dependencies: { other: '1.0.0' },
  }

  expect(await tryFastUpdateCatalogVersions(subject, updateOptions('^1.0.0'))).toBe('unsupported')
})

test('a manifest hook that renames the package falls back', async () => {
  const opts = updateOptions('^1.0.0') as unknown as Record<string, unknown>
  opts.readPackageHook = (manifest: { name: string }) => ({ ...manifest, name: 'renamed' })

  expect(await tryFastUpdateCatalogVersions(
    lockfile(),
    opts as unknown as Parameters<typeof tryFastUpdateCatalogVersions>[1]
  )).toBe('unsupported')
})

test('a satisfied range rides along with an exact move in one pass', async () => {
  const subject = lockfile()
  subject.catalogs!.default.child = { specifier: '1.1.0', version: '1.1.0' }
  const opts = updateOptions('^1.0.0') as unknown as Record<string, unknown>
  opts.catalogs = { default: { target: '2.0.0', child: '^1.1.0' } }

  expect(await tryFastUpdateCatalogVersions(
    subject,
    opts as unknown as Parameters<typeof tryFastUpdateCatalogVersions>[1]
  )).toBe('applied')
  expect(subject.catalogs!.default.child).toStrictEqual({ specifier: '^1.1.0', version: '1.1.0' })
  expect(subject.catalogs!.default.target).toStrictEqual({ specifier: '2.0.0', version: '2.0.0' })
})

test('a move on a package an override pins falls back', async () => {
  const opts = updateOptions('^1.0.0') as unknown as Record<string, unknown>
  opts.parsedOverrides = [{ selector: 'target', newBareSpecifier: '1.0.0', targetPkg: { name: 'target' } }]

  expect(await tryFastUpdateCatalogVersions(
    lockfile(),
    opts as unknown as Parameters<typeof tryFastUpdateCatalogVersions>[1]
  )).toBe('unsupported')
})

test('a range the locked version still satisfies moves no package', async () => {
  const subject = lockfile()

  expect(await tryFastUpdateCatalogVersions(subject, updateOptions('^1.0.0', '^1.0.0')))
    .toBe('nothing-to-move')
  // Nothing was written: with no package to move, the range-only rewrite owns
  // the specifier and this leaves the lockfile for it.
  expect(subject.catalogs!.default.target).toStrictEqual({ specifier: '1.0.0', version: '1.0.0' })
})

test('an unchanged catalog reports that nothing moved', async () => {
  expect(await tryFastUpdateCatalogVersions(lockfile(), updateOptions('^1.0.0', '1.0.0')))
    .toBe('nothing-to-move')
})
