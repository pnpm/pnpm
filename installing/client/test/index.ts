/// <reference path="../../../__typings__/index.d.ts"/>
import { afterAll, expect, test } from '@jest/globals'
import { createClient, createResolver } from '@pnpm/installing.client'
import { StoreIndex } from '@pnpm/store.index'

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

test('createClient()', () => {
  const storeIndex = new StoreIndex('.store')
  storeIndexes.push(storeIndex)
  const client = createClient({
    configByUri: {},
    cacheDir: '',
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir: '.store',
    storeIndex,
  })
  expect(typeof client === 'object').toBeTruthy()
})

test('createResolver()', () => {
  const { resolve } = createResolver({
    configByUri: {},
    cacheDir: '',
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir: '.store',
  })
  expect(typeof resolve === 'function').toBeTruthy()
})
