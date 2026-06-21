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

test('pkgSnapshotToResolution() converts git-hosted and file: tarball snapshots', () => {
  // The integrity requirement for registry tarballs is enforced by the npm
  // resolver's lockfile verifier, not here — this pure conversion returns
  // git-hosted (commit-anchored) and file: (local) tarballs as-is.
  expect(pkgSnapshotToResolution('foo@https+++github.com+foo+bar', {
    resolution: {
      tarball: 'https://codeload.github.com/foo/bar/tar.gz/abc1234',
      gitHosted: true,
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    tarball: 'https://codeload.github.com/foo/bar/tar.gz/abc1234',
    gitHosted: true,
  })

  expect(pkgSnapshotToResolution('is-negative@https+++codeload.github.com+kevva+is-negative+tar.gz+abc', {
    resolution: {
      tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/abc1234',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/abc1234',
  })

  // `file:` tarballs are local files; the user already controls the
  // bytes, and the install pipeline may write them without integrity.
  expect(pkgSnapshotToResolution('local-pkg@file:local-pkg-1.0.0.tgz', {
    resolution: {
      tarball: 'file:local-pkg-1.0.0.tgz',
    },
    version: '1.0.0',
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    tarball: 'file:local-pkg-1.0.0.tgz',
  })
})
