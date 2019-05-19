///<reference path="../../../typings/index.d.ts"/>
import createFetcher from '@pnpm/default-fetcher'
import test = require('tape')

test('createFetcher()', t => {
  const fetcher = createFetcher({
    alwaysAuth: false,
    rawNpmConfig: {},
    registry: 'https://registry.npmjs.org/',
    strictSsl: false,
  })
  t.equal(typeof fetcher, 'object')
  t.end()
})
