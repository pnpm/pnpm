import test = require('tape')
import createFetcher from '@pnpm/default-fetcher'

test('createFetcher()', t => {
  const fetcher = createFetcher({
    alwaysAuth: false,
    registry: 'https://registry.npmjs.org/',
    strictSsl: false,
    rawNpmConfig: {},
  })
  t.equal(typeof fetcher, 'object')
  t.end()
})
