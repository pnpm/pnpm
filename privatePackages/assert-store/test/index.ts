/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import assertStore from '@pnpm/assert-store'

test('assertStore() store assertions', async () => {
  const storePath = path.join(__dirname, 'fixture/store/v3/')
  const encodedRegistryName = 'registry.npmjs.org'
  const store = assertStore(storePath, encodedRegistryName)

  await store.storeHas('is-positive', '3.1.0')
  await store.storeHasNot('ansi-regex', '2.0.0')
  await store.storeHasNot('is-positive', '2.0.0')
})

test('assertStore() resolve', async () => {
  const storePath = path.join(__dirname, 'fixture/store/v3/')
  const encodedRegistryName = 'registry.npmjs.org'
  const store = assertStore(storePath, encodedRegistryName)

  expect(typeof await store.resolve('is-positive', '3.1.0')).toBe('string')
})
