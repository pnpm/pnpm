/// <reference path="../../../__typings__/index.d.ts"/>
import { createResolver } from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'

test('createResolver()', () => {
  const getAuthHeader = () => undefined
  const { resolve } = createResolver(createFetchFromRegistry({}), getAuthHeader, {
    cacheDir: '.cache',
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    rawConfig: {},
  })
  expect(typeof resolve).toEqual('function')
})
