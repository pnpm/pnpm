import { expect, test } from '@jest/globals'
import { catalogResolutionIsStale, catalogResolutionsAreUpToDate } from '@pnpm/lockfile.verification'

test('peer-suffixed catalog resolutions are compared by version', () => {
  const importer = {
    dependencies: {
      foo: '1.3.0(peer@1.0.0)',
    },
    specifiers: {
      foo: 'catalog:',
    },
  }
  const catalogs = {
    default: {
      foo: {
        specifier: '^1.4.0',
        version: '1.4.0',
      },
    },
  }

  expect(catalogResolutionIsStale({ importer, catalogs, alias: 'foo', specifier: 'catalog:' })).toBe(true)
  expect(catalogResolutionsAreUpToDate(importer, catalogs)).toBe(false)
})
