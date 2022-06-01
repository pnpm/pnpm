/// <reference path="../../../typings/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import getPort from 'get-port'
import createClient from '@pnpm/client'
import createStore from '@pnpm/package-store'
import { connectStoreController, createServer } from '@pnpm/server'
import fetch from 'node-fetch'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import tempy from 'tempy'
import isPortReachable from 'is-port-reachable'

const registry = 'https://registry.npmjs.org/'

async function createStoreController (storeDir?: string) {
  const tmp = tempy.directory()
  if (!storeDir) {
    storeDir = path.join(tmp, 'store')
  }
  const authConfig = { registry }
  const { resolve, fetchers } = createClient({
    authConfig,
    cacheDir: path.join(tmp, 'cache'),
  })
  return createStore(resolve, fetchers, {
    networkConcurrency: 1,
    storeDir,
    verifyStoreIntegrity: true,
  })
}

test('server', async () => {
  const port = await getPort()
  const hostname = 'localhost'
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

  expect((await response.bundledManifest!()).name).toBe('is-positive')
  expect(response.body.id).toBe('registry.npmjs.org/is-positive/1.0.0')

  expect(response.body.manifest!.name).toBe('is-positive')
  expect(response.body.manifest!.version).toBe('1.0.0')

  const files = await response.files!()
  expect(files.fromStore).toBeFalsy()
  expect(files.filesIndex).toHaveProperty(['package.json'])
  expect(response.finishing).toBeTruthy()

  await response.finishing!()

  await server.close()
  await storeCtrl.close()
})

test('fetchPackage', async () => {
  const port = await getPort()
  const hostname = 'localhost'
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
    pkg: {
      id: pkgId,
      resolution: {
        integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
        registry: 'https://registry.npmjs.org/',
        tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    },
  })

  expect(typeof response.filesIndexFile).toBe('string')

  expect(await response.bundledManifest!()).toBeTruthy()

  const files = await response['files']()
  expect(files.fromStore).toBeFalsy()
  expect(files.filesIndex).toHaveProperty(['package.json'])
  expect(response).toHaveProperty(['finishing'])

  await response['finishing']()

  await server.close()
  await storeCtrl.close()
})

test('server errors should arrive to the client', async () => {
  const port = await getPort()
  const hostname = 'localhost'
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
  } catch (e: any) { // eslint-disable-line
    caught = true
    expect(e.message).toBe('GET https://registry.npmjs.org/not-an-existing-package: Not Found - 404')
    expect(e.hint).toBe(`not-an-existing-package is not in the npm registry, or you have no permission to fetch it.

No authorization header was set for the request.`)
    expect(e.code).toBe('ERR_PNPM_FETCH_404')
    expect(e.response).toBeTruthy()
    expect(e.pkgName).toBeTruthy()
  }
  expect(caught).toBeTruthy()

  await server.close()
  await storeCtrl.close()
})

test('server upload', async () => {
  const port = await getPort()
  const hostname = 'localhost'
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
  expect(Object.keys(cacheIntegrity?.['sideEffects'][fakeEngine]).sort()).toStrictEqual(['side-effect.js', 'side-effect.txt'])

  await server.close()
  await storeCtrl.close()
})

test('disable server upload', async () => {
  await rimraf('.store')

  const port = await getPort()
  const hostname = 'localhost'
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
  expect(thrown).toBeTruthy()

  expect(fs.existsSync(filesIndexFile)).toBeFalsy()

  await server.close()
  await storeCtrl.close()
})

test('stop server with remote call', async () => {
  const port = await getPort()
  const hostname = 'localhost'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  createServer(storeCtrlForServer, {
    hostname,
    ignoreStopRequests: false,
    port,
  })

  expect(await isPortReachable(port)).toBeTruthy()

  const response = await fetch(`${remotePrefix}/stop`, { method: 'POST' })

  expect(response.status).toBe(200)

  expect(await isPortReachable(port)).toBeFalsy()
})

test('disallow stop server with remote call', async () => {
  const port = await getPort()
  const hostname = 'localhost'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    ignoreStopRequests: true,
    port,
  })

  expect(await isPortReachable(port)).toBeTruthy()

  const response = await fetch(`${remotePrefix}/stop`, { method: 'POST' })
  expect(response.status).toBe(403)

  expect(await isPortReachable(port)).toBeTruthy()

  await server.close()
})

test('disallow store prune', async () => {
  const port = await getPort()
  const hostname = 'localhost'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  expect(await isPortReachable(port)).toBeTruthy()

  const response = await fetch(`${remotePrefix}/prune`, { method: 'POST' })
  expect(response.status).toBe(403)

  await server.close()
  await storeCtrlForServer.close()
})

test('server should only allow POST', async () => {
  const port = await getPort()
  const hostname = 'localhost'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  expect(await isPortReachable(port)).toBeTruthy()

  // Try various methods (not including POST)
  const methods = ['GET', 'PUT', 'PATCH', 'DELETE', 'OPTIONS']

  for (const method of methods) {
    // Ensure 405 error is received
    const response = await fetch(`${remotePrefix}/a-random-endpoint`, { method: method })
    expect(response.status).toBe(405)
    expect((await response.json() as any).error).toBeTruthy() // eslint-disable-line
  }

  await server.close()
  await storeCtrlForServer.close()
})

test('server route not found', async () => {
  const port = await getPort()
  const hostname = 'localhost'
  const remotePrefix = `http://${hostname}:${port}`
  const storeCtrlForServer = await createStoreController()
  const server = createServer(storeCtrlForServer, {
    hostname,
    port,
  })

  expect(await isPortReachable(port)).toBeTruthy()

  // Ensure 404 error is received
  const response = await fetch(`${remotePrefix}/a-random-endpoint`, { method: 'POST' })
  // Ensure error is correct
  expect(response.status).toBe(404)
  expect((await response.json() as any).error).toBeTruthy() // eslint-disable-line

  await server.close()
  await storeCtrlForServer.close()
})
