import { pkgSnapshotToResolution } from '@pnpm/lockfile-utils'
import test = require('tape')

test('pkgSnapshotToResolution()', (t) => {
  t.deepEqual(pkgSnapshotToResolution('/foo/1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, { default: 'https://registry.npmjs.org/' }), {
    integrity: 'AAAA',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })

  t.deepEqual(pkgSnapshotToResolution('/@mycompany/mypackage/2.0.0', {
    resolution: {
      integrity: 'AAAA',
      tarball: '@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
    },
  }, { default: 'https://registry.npmjs.org/', '@mycompany': 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/' }), {
    integrity: 'AAAA',
    registry: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/',
    tarball: 'https://mycompany.jfrog.io/mycompany/api/npm/npm-local/@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz',
  })

  t.end()
})
