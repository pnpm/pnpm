///<reference path="../../../typings/index.d.ts"/>
import createResolver, { PackageMetaCache } from '@pnpm/npm-resolver'
import createStore from '@pnpm/package-store'
import { connectStoreController, createServer, } from '@pnpm/server'
import { PackageFilesResponse, ResolveFunction } from '@pnpm/store-controller-types'
import createFetcher from '@pnpm/tarball-fetcher'
import rimraf = require('@zkochan/rimraf')
import isPortReachable = require('is-port-reachable')
import fs = require('mz/fs')
import fetch from 'node-fetch'
import path = require('path')
import test = require('tape')

const registry = 'https://registry.npmjs.org/'

const store = '.store'

async function createStoreController () {
  const rawNpmConfig = { registry }
  const resolve = createResolver({
    metaCache: new Map<string, object>() as PackageMetaCache,
    rawNpmConfig,
    store,
  }) as ResolveFunction
  const fetchers = createFetcher({
    alwaysAuth: true,
    rawNpmConfig,
    registry,
    strictSsl: true,
  })
  return createStore(resolve, fetchers, {
    locks: undefined,
    lockStaleDuration: 100,
    networkConcurrency: 1,
    store: store,
    verifyStoreIntegrity: true,
  })
}

test('server', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })
  const prefix = process.cwd()
  const response = await storeCtrl.requestPackage(
    { alias: 'is-positive', pref: '1.0.0' },
    {
      downloadPriority: 0,
      lockfileDirectory: prefix,
      preferredVersions: {},
      prefix,
      registry,
      sideEffectsCache: false,
    }
  )

  t.equal((await response.bundledManifest!()).name, 'is-positive', 'responded with bundledManifest')
  t.equal(response.body.id, 'registry.npmjs.org/is-positive/1.0.0', 'responded with correct ID')

  t.equal(response.body.manifest!.name, 'is-positive', 'responded with correct name in manifest')
  t.equal(response.body.manifest!.version, '1.0.0', 'responded with correct version in manifest')

  const files = await response.files!()
  t.notOk(files.fromStore)
  t.ok(files.filenames.includes('package.json'))
  t.ok(response.finishing)

  await response.finishing!()

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('fetchPackage', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const response = await storeCtrl.fetchPackage({
    fetchRawManifest: true,
    force: false,
    pkgId,
    prefix: process.cwd(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })

  t.equal(typeof response.inStoreLocation, 'string', 'location in store returned')

  t.ok(await response.bundledManifest!())

  const files = await response['files']() as PackageFilesResponse
  t.notOk(files.fromStore)
  t.ok(files.filenames.includes('package.json'))
  t.ok(response['finishing'])

  await response['finishing']()

  t.comment('getPackageLocation()')

  t.deepEqual(
    await storeCtrl.getPackageLocation(pkgId, 'is-positive', {
      lockfileDirectory: process.cwd(),
    }),
    {
      directory: path.join(store, pkgId, 'node_modules', 'is-positive'),
      isBuilt: false,
    },
  )

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('server errors should arrive to the client', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })
  let caught = false
  try {
    const prefix = process.cwd()
    await storeCtrl.requestPackage(
      { alias: 'not-an-existing-package', pref: '1.0.0' },
      {
        downloadPriority: 0,
        lockfileDirectory: prefix,
        preferredVersions: {},
        prefix,
        registry,
        sideEffectsCache: false,
      }
    )
  } catch (e) {
    caught = true
    t.equal(e.message, '404 Not Found: not-an-existing-package (via https://registry.npmjs.org/not-an-existing-package)', 'error message delivered correctly')
    t.equal(e.code, 'ERR_PNPM_REGISTRY_META_RESPONSE_404', 'error code delivered correctly')
    t.ok(e.uri, 'error uri field delivered')
    t.ok(e.response, 'error response field delivered')
    t.ok(e.package, 'error package field delivered')
  }
  t.ok(caught, 'exception raised correctly')

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('server upload', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })

  const fakeEngine = 'client-engine'
  const fakePkgId = 'test.example.com/fake-pkg/1.0.0'

  await storeCtrl.upload(path.join(__dirname, 'side-effect-fake-dir'), {
    engine: fakeEngine,
    packageId: fakePkgId,
  })

  const cachePath = path.join('.store', fakePkgId, 'side_effects', fakeEngine, 'package')
  t.ok(await fs.exists(cachePath), 'cache directory created')
  t.deepEqual(await fs.readdir(cachePath), ['side-effect.js', 'side-effect.txt'], 'all files uploaded to cache')

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('disable server upload', async t => {
  await rimraf('.store')

  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    ignoreUploadRequests: true,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })

  const fakeEngine = 'client-engine'
  const fakePkgId = 'test.example.com/fake-pkg/1.0.0'

  let thrown = false
  try {
    await storeCtrl.upload(path.join(__dirname, 'side-effect-fake-dir'), {
      engine: fakeEngine,
      packageId: fakePkgId,
    })
  } catch (e) {
    thrown = true
  }
  t.ok(thrown, 'error is thrown when trying to upload')

  const cachePath = path.join('.store', fakePkgId, 'side_effects', fakeEngine, 'package')
  t.notOk(await fs.exists(cachePath), 'cache directory not created')

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('stop server with remote call', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    ignoreStopRequests: false,
    port,
  })

  t.ok(await isPortReachable(port), 'server is running')

  const response = await fetch(`${remotePrefix}/stop`, { method: 'POST' })

  t.equal(response.status, 200, 'success returned by server stopping endpoint')

  t.notOk(await isPortReachable(port), 'server is not running')

  t.end()
})

test('disallow stop server with remote call', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    ignoreStopRequests: true,
    port,
  })

  t.ok(await isPortReachable(port), 'server is running')

  const response = await fetch(`${remotePrefix}/stop`, { method: 'POST' })
  t.equal(response.status, 403, 'server not stopped')

  t.ok(await isPortReachable(port), 'server is running')

  await server.close()
  t.end()
})

test('disallow store prune', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  t.ok(await isPortReachable(port), 'server is running')

  const response = await fetch(`${remotePrefix}/prune`, { method: 'POST' })
  t.equal(response.status, 403, 'store not pruned')

  await server.close()
  await storeCtrlForServer.close()
  t.end()
})

test('find package usages', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })

  const dependency = { alias: 'is-positive', pref: '1.0.0' }

  const prefix = process.cwd()
  // First install a dependency
  const requestResponse = await storeCtrl.requestPackage(
    dependency,
    {
      downloadPriority: 0,
      lockfileDirectory: prefix,
      preferredVersions: {},
      prefix,
      registry,
      sideEffectsCache: false,
    }
  )
  await requestResponse.bundledManifest!()
  await requestResponse.finishing!()

  // For debugging purposes
  await storeCtrl.saveState()

  // Now check if usages shows up
  const packageUsagesByPackageSelectors = await storeCtrl.findPackageUsages(['/is-positive/1.0.0'])

  t.deepEqual(
    packageUsagesByPackageSelectors,
    {
      '/is-positive/1.0.0': [
        {
          packageId: 'registry.npmjs.org/is-positive/1.0.0',
          usages: [],
        },
      ],
    },
  )

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('server should only allow POST', async (t) => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  t.ok(await isPortReachable(port), 'server is running')

  // Try various methods (not including POST)
  const methods = ['GET', 'PUT', 'PATCH', 'DELETE', 'OPTIONS']

  for (let method of methods) {
    t.comment(`Testing HTTP ${method}`)
    // Ensure 405 error is received
    const response = await fetch(`${remotePrefix}/a-random-endpoint`, { method: method })
    t.equal(response.status, 405, 'response code should be a 504')
    t.ok((await response.json()).error, 'error field should be set in response body')
  }

  await server.close()
  await storeCtrlForServer.close()
  t.end()
})

test('server route not found', async (t) => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  t.ok(await isPortReachable(port), 'server is running')

  // Ensure 404 error is received
  const response = await fetch(`${remotePrefix}/a-random-endpoint`, { method: 'POST' })
  // Ensure error is correct
  t.equal(response.status, 404, 'response code should be a 404')
  t.ok((await response.json()).error, 'error field should be set in response body')

  await server.close()
  await storeCtrlForServer.close()
  t.end()
})
