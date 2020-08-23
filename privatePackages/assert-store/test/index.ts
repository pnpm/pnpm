/// <reference path="../../../typings/index.d.ts"/>
import assertStore from '@pnpm/assert-store'
import path = require('path')
import test = require('tape')

test('assertStore() store assertions', async (t) => {
  const storePath = path.join(__dirname, 'fixture/store/v3/')
  const encodedRegistryName = 'registry.npmjs.org'
  const store = assertStore(t, storePath, encodedRegistryName)

  await store.storeHas('is-positive', '3.1.0')
  await store.storeHasNot('ansi-regex', '2.0.0')
  await store.storeHasNot('is-positive', '2.0.0')

  t.end()
})

test('assertStore() resolve', async (t) => {
  const storePath = path.join(__dirname, 'fixture/store/v3/')
  const encodedRegistryName = 'registry.npmjs.org'
  const store = assertStore(t, storePath, encodedRegistryName)

  t.equal(typeof await store.resolve('is-positive', '3.1.0'), 'string')
  t.end()
})
