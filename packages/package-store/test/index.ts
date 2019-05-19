///<reference path="../../../typings/index.d.ts"/>
import createResolver from '@pnpm/npm-resolver'
import createStore, * as packageStore from '@pnpm/package-store'
import { ResolveFunction } from '@pnpm/resolver-base'
import createFetcher from '@pnpm/tarball-fetcher'
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

test('public API', t => {
  t.equal(typeof packageStore.read, 'function')
  t.end()
})

test('store.importPackage()', async (t) => {
  const store = tempy.directory()
  const registry = 'https://registry.npmjs.org/'
  const rawNpmConfig = { registry }
  const resolver = createResolver({
    metaCache: new Map(),
    rawNpmConfig,
    store,
  }) as ResolveFunction
  const fetcher = createFetcher({
    rawNpmConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {
    store,
    verifyStoreIntegrity: true,
  })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResult = await storeController.fetchPackage({
    force: false,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })
  const importTo = tempy.directory()
  const importFrom = path.join(fetchResult.inStoreLocation, 'node_modules', 'is-positive')
  await storeController.importPackage(importFrom, importTo, {
    filesResponse: await fetchResult.fetchingFiles,
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})

test('store.importPackage() by copying', async (t) => {
  const store = tempy.directory()
  const registry = 'https://registry.npmjs.org/'
  const rawNpmConfig = { registry }
  const resolver = createResolver({
    metaCache: new Map(),
    rawNpmConfig,
    store,
  }) as ResolveFunction
  const fetcher = createFetcher({
    rawNpmConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {
    packageImportMethod: 'copy',
    store,
    verifyStoreIntegrity: true,
  })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResult = await storeController.fetchPackage({
    force: false,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    },
  })
  const importTo = tempy.directory()
  const importFrom = path.join(fetchResult.inStoreLocation, 'node_modules', 'is-positive')
  await storeController.importPackage(importFrom, importTo, {
    filesResponse: await fetchResult.fetchingFiles,
    force: false,
  })
  t.equal(typeof require(importTo), 'function', `sucessfully imported to ${importTo}`)
  t.end()
})
