/// <reference path="../../../typings/index.d.ts"/>
import createClient from '@pnpm/client'
import createStore from '@pnpm/package-store'
import { connectStoreController, createServer } from '@pnpm/server'
import fetch from 'node-fetch'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import isPortReachable = require('is-port-reachable')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import test = require('tape')
import tempy = require('tempy')

const registry = 'https://registry.npmjs.org/'

function createStoreController (storeDir?: string) {
  if (!storeDir) {
    storeDir = tempy.directory()
  }
  const authConfig = { registry }
  const { resolve, fetchers } = createClient({
    authConfig,
    storeDir,
  })
  return createStore(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir,
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
  const projectDir = process.cwd()
  const response = await storeCtrl.requestPackage(
    { alias: 'is-positive', pref: '1.0.0' },
    {
      downloadPriority: 0,
      lockfileDir: projectDir,
      preferredVersions: {},
      projectDir,
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
  t.ok(files.filesIndex['package.json'])
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
  const storeDir = tempy.directory()
  const storeCtrlForServer = await createStoreController(storeDir)
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const response = await storeCtrl.fetchPackage({
    fetchRawManifest: true,
    force: false,
    lockfileDir: process.cwd(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })

  t.equal(typeof response.filesIndexFile, 'string', 'index file location in store returned')

  t.ok(await response.bundledManifest!())

  const files = await response['files']()
  t.notOk(files.fromStore)
  t.ok(files.filesIndex['package.json'])
  t.ok(response['finishing'])

  await response['finishing']()

  t.comment('getPackageLocation()')

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
    const projectDir = process.cwd()
    await storeCtrl.requestPackage(
      { alias: 'not-an-existing-package', pref: '1.0.0' },
      {
        downloadPriority: 0,
        lockfileDir: projectDir,
        preferredVersions: {},
        projectDir,
        registry,
        sideEffectsCache: false,
      }
    )
  } catch (e) {
    caught = true
    t.equal(e.message, 'GET https://registry.npmjs.org/not-an-existing-package: Not Found - 404', 'error message delivered correctly')
    t.equal(e.hint, `not-an-existing-package is not in the npm registry, or you have no permission to fetch it.

No authorization header was set for the request.`)
    t.equal(e.code, 'ERR_PNPM_FETCH_404', 'error code delivered correctly')
    t.ok(e.response, 'error response field delivered')
    t.ok(e.pkgName, 'error package field delivered')
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
  const storeDir = tempy.directory()
  const storeCtrlForServer = await createStoreController(storeDir)
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })
  const storeCtrl = await connectStoreController({ remotePrefix, concurrency: 100 })

  const fakeEngine = 'client-engine'
  const filesIndexFile = path.join(storeDir, 'test.example.com/fake-pkg/1.0.0.json')

  await storeCtrl.upload(path.join(__dirname, 'side-effect-fake-dir'), {
    engine: fakeEngine,
    filesIndexFile,
  })

  const cacheIntegrity = await loadJsonFile(filesIndexFile)
  t.deepEqual(Object.keys(cacheIntegrity['sideEffects'][fakeEngine]).sort(), ['side-effect.js', 'side-effect.txt'], 'all files uploaded to cache')

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
  const storeDir = tempy.directory()
  const filesIndexFile = path.join(storeDir, 'test.example.com/fake-pkg/1.0.0.json')

  let thrown = false
  try {
    await storeCtrl.upload(path.join(__dirname, 'side-effect-fake-dir'), {
      engine: fakeEngine,
      filesIndexFile,
    })
  } catch (e) {
    thrown = true
  }
  t.ok(thrown, 'error is thrown when trying to upload')

  t.notOk(await fs.exists(filesIndexFile), 'cache directory not created')

  await server.close()
  await storeCtrl.close()
  t.end()
})

test('stop server with remote call', async t => {
  const port = 5813
  const hostname = '127.0.0.1'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  createServer(storeCtrlForServer, {
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

  for (const method of methods) {
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
