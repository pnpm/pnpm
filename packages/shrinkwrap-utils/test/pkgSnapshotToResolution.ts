import { pkgSnapshotToResolution } from '@pnpm/shrinkwrap-utils'
import test = require('tape')

test('pkgSnapshotToResolution()', (t) => {
  t.deepEqual(pkgSnapshotToResolution('/foo/1.0.0', {
    resolution: {
      integrity: 'AAAA',
    },
  }, 'https://registry.npmjs.org/'), {
    integrity: 'AAAA',
    registry: 'https://registry.npmjs.org/',
    tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
  })

  t.end()
})
