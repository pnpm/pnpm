/// <reference path="../../../__typings__/index.d.ts"/>
import { createClient, createResolver } from '@pnpm/client'
import { StoreIndex } from '@pnpm/store.index'

test('createClient()', () => {
  const storeIndex = new StoreIndex('.store')
  const client = createClient({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir: '.store',
    storeIndex,
  })
  storeIndex.close()
  expect(typeof client === 'object').toBeTruthy()
})

test('createResolver()', () => {
  const { resolve } = createResolver({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir: '.store',
  })
  expect(typeof resolve === 'function').toBeTruthy()
})
