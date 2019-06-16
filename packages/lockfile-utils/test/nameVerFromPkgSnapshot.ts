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

  t.deepEqual(nameVerFromPkgSnapshot('/foo/1.0.0_aaa', {
    resolution: {
      integrity: 'AAA',
    },
  }), {
    name: 'foo',
    peersSuffix: 'aaa',
    version: '1.0.0',
  })

  t.end()
})
