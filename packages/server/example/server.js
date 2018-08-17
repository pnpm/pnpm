'use strict'
const createServer = require('@pnpm/server').createServer
const createStore = require('package-store').default
const createResolver = require('@pnpm/npm-resolver').default
const createFetcher = require('@pnpm/tarball-fetcher').default

main()
  .then(() => console.log('Server started'))
  .catch(err => console.error(err))

async function main() {
  const registry = 'https://registry.npmjs.org/'
  const rawNpmConfig = { registry }
  const store = '.store'
  const resolve = createResolver({
    rawNpmConfig,
    store,
    metaCache: new Map(),
  })
  const fetchers = createFetcher({
    alwaysAuth: true,
    registry,
    strictSsl: true,
    rawNpmConfig,
  })
  const storeCtrl = await createStore(resolve, fetchers, {
    networkConcurrency: 1,
    store,
  })

  const port = 5813
  const hostname = '127.0.0.1';
  const server = createServer(storeCtrl, {
    port,
    hostname,
  })

  process.on('exit', () => server.close())
}
