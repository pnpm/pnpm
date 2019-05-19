///<reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/default-resolver'
import test = require('tape')

test('createResolver()', t => {
  const resolve = createResolver({
    metaCache: new Map(),
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org/',
    },
    store: '.store',
  })
  t.equal(typeof resolve, 'function')
  t.end()
})
