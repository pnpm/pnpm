import test = require('tape')
import createResolver from '@pnpm/default-resolver'

test('createResolver()', t => {
  const resolve = createResolver({
    metaCache: new Map(),
    store: '.store',
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org/',
    },
  })
  t.equal(typeof resolve, 'function')
  t.end()
})
