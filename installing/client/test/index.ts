/// <reference path="../../../__typings__/index.d.ts"/>
import { createClient, createResolver } from '@pnpm/installing.client'
import { StoreIndex } from '@pnpm/store.index'

const storeIndexes: StoreIndex[] = []
after all(() => {
  for (const si of storeIndexes) si.close()
})

test('createClient()', () => {
  const storeIndex = new StoreIndex('.store')
  storeIndexes.push(storeIndex)
  const client = createClient({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
    storeIndex,
  })
  expect(typeof client === 'object').toBeTruthy()
})

test('createResolver()', () => {
  const { resolve } = createResolver({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
  })
  expect(typeof resolve === 'function').toBeTruthy()
})
