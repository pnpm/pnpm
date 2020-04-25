///<reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/npm-resolver'
import createStore, * as packageStore from '@pnpm/package-store'
import { ResolveFunction } from '@pnpm/resolver-base'
import createFetcher from '@pnpm/tarball-fetcher'
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import './createImportPackage.spec'

test('public API', t => {
  t.equal(typeof packageStore.read, 'function')
  t.end()
})

test('store.importPackage()', async (t) => {
  const storeDir = tempy.directory()
  const registry = 'https://registry.npmjs.org/'
  const rawConfig = { registry }
  const resolver = createResolver({
    metaCache: new Map(),
    rawConfig,
    storeDir,
  }) as ResolveFunction
  const fetcher = createFetcher({
    rawConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {
    storeDir,
    verifyStoreIntegrity: true,
  })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResponse = storeController.fetchPackage({
    force: false,
    lockfileDir: tempy.directory(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })
  const importTo = tempy.directory()
  await storeController.importPackage(importTo, {
    filesResponse: await fetchResponse.files(),
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})

test('store.importPackage() by copying', async (t) => {
  const storeDir = tempy.directory()
  const registry = 'https://registry.npmjs.org/'
  const rawConfig = { registry }
  const resolver = createResolver({
    metaCache: new Map(),
    rawConfig,
    storeDir,
  }) as ResolveFunction
  const fetcher = createFetcher({
    rawConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {
    packageImportMethod: 'copy',
    storeDir,
    verifyStoreIntegrity: true,
  })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResponse = storeController.fetchPackage({
    force: false,
    lockfileDir: tempy.directory(),
    pkgId,
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })
  const importTo = tempy.directory()
  await storeController.importPackage(importTo, {
    filesResponse: await fetchResponse.files(),
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})
