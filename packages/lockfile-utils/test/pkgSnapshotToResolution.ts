import { pkgSnapshotToResolution } from '@pnpm/lockfile-utils'

test('pkgSnapshotToResolution()', () => {
  expect(pkgSnapshotToResolution('/foo/1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    integrity: 'AAAA',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('/@mycompany/mypackage/2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/' })).toEqual({
    integrity: 'AAAA',
    registry: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  expect(pkgSnapshotToResolution('/foo/1.0.0', {
    resolution: {
      integrity: 'AAAA',
      registry: 'https://npm.pkg.github.com/',
      tarball: 'https://npm.pkg.github.com/download/@foo/bar/1.0.0/aaa',
    },
  }, { default: 'https://registry.npmjs.org/' })).toEqual({
    integrity: 'AAAA',
    registry: 'https://npm.pkg.github.com/',
    tarball: 'https://npm.pkg.github.com/download/@foo/bar/1.0.0/aaa',
  })
})
