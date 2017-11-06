import * as packageStore from 'package-store'
import test = require('tape')
import './network/got'

test('public API', t => {
  t.equal(typeof packageStore.createGot, 'function')
  t.equal(typeof packageStore.fetch, 'function')
  t.equal(typeof packageStore.getRegistryName, 'function')
  t.equal(typeof packageStore.pkgIdToFilename, 'function')
  t.equal(typeof packageStore.pkgIsUntouched, 'function')
  t.equal(typeof packageStore.read, 'function')
  t.end()
})
