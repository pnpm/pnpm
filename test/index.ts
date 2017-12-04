import test = require('tape')
import createPackageRequester from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'

const resolve = createResolver({rawNpmConfig: {}})
const fetch = createFetcher({
  alwaysAuth: false,
  registry: 'https://registry.npmjs.org/',
  strictSsl: false,
  rawNpmConfig: {},
})

test('createPackageRequester', t => {
  const requestPackage = createPackageRequester(resolve, fetch, {networkConcurrency: 1})
  t.equal(typeof requestPackage, 'function')
  t.end()
})
