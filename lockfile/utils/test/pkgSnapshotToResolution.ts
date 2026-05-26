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
      integrity: 'sha512-CCCC',
      tarball: 'https://cdn.sheetjs.com/xlsx-0.18.9/xlsx-0.18.9.tgz',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    integrity: 'sha512-CCCC',
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

test('pkgSnapshotToResolution() rejects a tarball-shaped resolution that has no integrity', () => {
  // A tampered or malformed lockfile that strips the `integrity` field
  // would otherwise let pnpm download the URL contents unchecked. The
  // helper must fail closed so neither install path nor any read-only
  // consumer (sbom, list, etc.) silently trusts the lockfile entry.
  expect(() => pkgSnapshotToResolution('foo@1.0.0', {
    resolution: {
      tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
    },
  }, { default: 'https://registry.npmjs.org/' })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_MISSING_TARBALL_INTEGRITY' }))

  // Git-hosted and `file:` tarballs follow the same rule: even though their
  // resolutions are returned as-is rather than rewritten, missing integrity
  // is still a verification gap.
  expect(() => pkgSnapshotToResolution('foo@https+++github.com+foo+bar', {
    resolution: {
      tarball: 'https://codeload.github.com/foo/bar/tar.gz/abc',
      gitHosted: true,
    },
  }, { default: 'https://registry.npmjs.org/' })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_MISSING_TARBALL_INTEGRITY' }))

  expect(() => pkgSnapshotToResolution('local-pkg@file:local-pkg-1.0.0.tgz', {
    resolution: {
      tarball: 'file:local-pkg-1.0.0.tgz',
    },
    version: '1.0.0',
  }, { default: 'https://registry.npmjs.org/' })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_MISSING_TARBALL_INTEGRITY' }))
})
