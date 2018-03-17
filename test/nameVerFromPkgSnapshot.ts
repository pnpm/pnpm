import {nameVerFromPkgSnapshot} from 'pnpm-shrinkwrap'
import test = require('tape')

test('nameVerFromPkgSnapshot()', (t) => {
  t.deepEqual(nameVerFromPkgSnapshot('/some-weird-path', {
    name: 'foo',
    version: '1.0.0',
  }), {
    name: 'foo',
    version: '1.0.0',
  })

  t.deepEqual(nameVerFromPkgSnapshot('/foo/1.0.0', {}), {
    name: 'foo',
    version: '1.0.0',
  })

  t.end()
})
