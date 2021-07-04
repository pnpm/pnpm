/// <reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'

test('createResolver()', () => {
  const getCredentials = () => ({ authHeaderValue: '', alwaysAuth: false })
  const resolve = createResolver(createFetchFromRegistry({}), getCredentials, {
    cacheDir: '.cache',
  })
  expect(typeof resolve).toEqual('function')
})
