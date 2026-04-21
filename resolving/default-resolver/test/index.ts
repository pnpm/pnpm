/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createResolver } from '@pnpm/resolving.default-resolver'

test('createResolver()', () => {
  const getAuthHeader = () => undefined
  const { resolve } = createResolver(createFetchFromRegistry({}), getAuthHeader, {
    cacheDir: '.cache',
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
  })
  expect(typeof resolve).toBe('function')
})
