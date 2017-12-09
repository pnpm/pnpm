import test = require('tape')
import {
  createServer,
  connectPackageRequester,
 } from '@pnpm/server'
import createPackageRequester, {
  PackageFilesResponse,
} from '@pnpm/package-requester'
import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'
import net = require('net')
import JsonSocket = require('json-socket')

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
  const requestPackageForServer = createPackageRequester(resolve, fetchers, {
    networkConcurrency: 1,
    storePath: store,
    storeIndex: {},
  })

  const port = 5813
  const hostname = '127.0.0.1';
  const server = createServer(requestPackageForServer, {
    port,
    hostname,
  })
  const requestPackage = await connectPackageRequester({port, hostname})
  const response = await requestPackage(
    {alias: 'is-positive', pref: '1.0.0'},
    {
      downloadPriority: 0,
      loggedPkg: {rawSpec: 'sfdf'},
      offline: false,
      prefix: process.cwd(),
      registry,
      verifyStoreIntegrity: false,
    }
  )

  t.equal(response.id, 'registry.npmjs.org/is-positive/1.0.0')

  const manifest = await response.fetchingManifest
  t.equal(manifest.name, 'is-positive')
  t.equal(manifest.version, '1.0.0')

  const files = await response['fetchingFiles'] as PackageFilesResponse
  t.notOk(files.fromStore)
  t.ok(files.filenames.indexOf('package.json') !== -1)
  t.ok(response['finishing'])

  await response['finishing']

  server.close()
  requestPackage['end']() // tslint:disable-line
  t.end()
})
