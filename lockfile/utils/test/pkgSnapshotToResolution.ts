import { expect, test } from '@jest/globals'
import { pkgSnapshotToResolution } from '@pnpm/lockfile.utils'

test('pkgSnapshotToResolution()', () => {
  expect(pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@mycompany/mypackage@2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/' })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@mycompany/mypackage@2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local' })).toEqual({
    integrity: 'AAAA',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('@cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz', {
    resolution: {
      tarball: 'https://cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    tarball: 'https://cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz',
  })

  // Snapshot for a `file:` dependency whose resolution lacks the tarball
  // field — the tarball should be recovered from the depPath.
  expect(pkgSnapshotToResolution('test-package@file:test-package-1.0.0.tgz', {
    resolution: {
      integrity: 'sha512-AAAA',
    },
    version: '1.0.0',
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    integrity: 'sha512-AAAA',
    tarball: 'file:test-package-1.0.0.tgz',
  })
})
