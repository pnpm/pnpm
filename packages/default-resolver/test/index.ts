///<reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/default-resolver'
import test = require('tape')

test('createResolver()', t => {
  const resolve = createResolver({
    metaCache: new Map(),
    rawConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    storeDir: '.store',
  })
  t.equal(typeof resolve, 'function')
  t.end()
})
