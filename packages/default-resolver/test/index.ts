///<reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/default-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'
import test = require('tape')

test('createResolver()', t => {
  const resolve = createResolver(createFetchFromRegistry({}), {
    metaCache: new Map(),
    rawConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
  })
  t.equal(typeof resolve, 'function')
  t.end()
})
