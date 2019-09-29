import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import test = require('tape')

test('nameVerFromPkgSnapshot()', (t) => {
  t.deepEqual(nameVerFromPkgSnapshot('/some-weird-path', {
    name: 'foo',
    version: '1.0.0',

    resolution: {
      integrity: 'AAA',
    },
  }), {
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(nameVerFromPkgSnapshot('/foo/1.0.0', {
    resolution: {
      integrity: 'AAA',
    },
  }), {
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(nameVerFromPkgSnapshot('/foo/1.0.0_bar@2.0.0', {
    resolution: {
      integrity: 'AAA',
    },
  }), {
    name: 'foo',
    peersSuffix: 'bar@2.0.0',
    version: '1.0.0',
  })

  t.end()
})
