import test = require('tape')
import createPackageRequester from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'

const registry = 'https://registry.npmjs.org/'

const rawNpmConfig = { registry }

const resolve = createResolver({
  rawNpmConfig,
  metaCache: new Map(),
  store: '.store',
})
const fetch = createFetcher({
  alwaysAuth: false,
  registry: 'https://registry.npmjs.org/',
  strictSsl: false,
  rawNpmConfig,
})

test('createPackageRequester', t => {
  const requestPackage = createPackageRequester(resolve, fetch, {
    networkConcurrency: 1,
    storePath: '.store',
    storeIndex: {},
  })
  t.equal(typeof requestPackage, 'function')
  t.end()
})
