/// <reference path="../../../__typings__/index.d.ts"/>
import { createClient, createResolver } from '@pnpm/client'

test('createClient()', () => {
  const client = createClient({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
  })
  expect(typeof client === 'object').toBeTruthy()
})

test('createResolver()', () => {
  const resolver = createResolver({
    authConfig: { registry: 'https://registry.npmjs.org/' },
    cacheDir: '',
    rawConfig: {},
  })
  expect(typeof resolver === 'function').toBeTruthy()
})
