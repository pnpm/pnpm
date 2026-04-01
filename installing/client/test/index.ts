/// <reference path="../../../__typings__/index.d.ts"/>
import { closeAllMetadataCaches } from '@pnpm/cache.metadata'
import { createClient, createResolver } from '@pnpm/installing.client'
import { StoreIndex } from '@pnpm/store.index'
import { temporaryDirectory } from 'tempy'

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  closeAllMetadataCaches()
  for (const si of storeIndexes) si.close()
})

test('createClient()', () => {
  const storeDir = temporaryDirectory()
  const storeIndex = new StoreIndex(storeDir)
  storeIndexes.push(storeIndex)
  const client = createClient({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: temporaryDirectory(),
    rawConfig: {},
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir,
    storeIndex,
  })
  expect(typeof client === 'object').toBeTruthy()
})

test('createResolver()', () => {
  const { resolve } = createResolver({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: temporaryDirectory(),
    rawConfig: {},
    registries: {
      default: 'https://reigstry.npmjs.org/',
    },
    storeDir: temporaryDirectory(),
  })
  expect(typeof resolve === 'function').toBeTruthy()
})
