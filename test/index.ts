import test = require('tape')
import createResolver from '@pnpm/default-resolver'

test('createResolver()', t => {
  const resolve = createResolver({rawNpmConfig: {}})
  t.equal(typeof resolve, 'function')
  t.end()
})
