import {pkgSnapshotToResolution} from 'pnpm-shrinkwrap'
import test = require('tape')

test('pkgSnapshotToResolution()', (t) => {
  t.deepEqual(pkgSnapshotToResolution('/foo/1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, 'https://registry.npmjs.org/'), {
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
    integrity: 'AAAA',
    registry: 'https://registry.npmjs.org/',
  })

  t.end()
})
