import test = require('tape')
import {
  createServer,
  connectStoreController,
 } from '@pnpm/server'
import {
  PackageFilesResponse,
} from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'
import createStore from 'package-store'

test('server', async t => {
  const registry = 'https://registry.npmjs.org/'
  const rawNpmConfig = { registry }
  const store = '.store'
  const resolve = createResolver({
    rawNpmConfig,
    store,
    metaCache: new Map<string, object>(),
  })
  const fetchers = createFetcher({
    alwaysAuth: true,
    registry,
    strictSsl: true,
    rawNpmConfig,
  })
  const storeCtrlForServer = await createStore(resolve, fetchers, {
    networkConcurrency: 1,
    store: store,
    locks: undefined,
    lockStaleDuration: 100,
  })

  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const server = createServer(storeCtrlForServer, {
    port,
    hostname,
  })
  const storeCtrl = await connectStoreController({remotePrefix, concurrency: 100})
  const response = await storeCtrl.requestPackage(
    {alias: 'is-positive', pref: '1.0.0'},
    {
      downloadPriority: 0,
      loggedPkg: {rawSpec: 'sfdf'},
      offline: false,
      prefix: process.cwd(),
      registry,
      verifyStoreIntegrity: false,
      preferredVersions: {},
    }
  )

  t.equal(response.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct ID')

  t.equal(response.body['manifest'].name, 'is-positive', 'responded with correct name in manifest')
  t.equal(response.body['manifest'].version, '1.0.0', 'responded with correct version in manifest')

  const files = await response['fetchingFiles'] as PackageFilesResponse
  t.notOk(files.fromStore)
  t.ok(files.filenames.indexOf('package.json') !== -1)
  t.ok(response['finishing'])

  await response['finishing']

  server.close()
  await storeCtrl.close()
  t.end()
})
