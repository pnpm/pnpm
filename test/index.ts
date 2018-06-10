import createResolver from '@pnpm/npm-resolver'
import createFetcher from '@pnpm/tarball-fetcher'
import createStore from 'package-store'
import * as packageStore from 'package-store'
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

test('public API', t => {
  t.equal(typeof packageStore.getRegistryName, 'function')
  t.equal(typeof packageStore.read, 'function')
  t.end()
})

test('store.importPackage()', async (t) => {
  const store = tempy.directory()
  const registry = 'https://registry.npmjs.org/'
  const rawNpmConfig = {registry}
  const resolver = createResolver({
    metaCache: new Map(),
    rawNpmConfig,
    store,
  })
  const fetcher = createFetcher({
    rawNpmConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {store})
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResult = await storeController.fetchPackage({
    force: false,
    verifyStoreIntegrity: true,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    }
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
  const rawNpmConfig = {registry}
  const resolver = createResolver({
    metaCache: new Map(),
    rawNpmConfig,
    store,
  })
  const fetcher = createFetcher({
    rawNpmConfig,
    registry,
  })
  const storeController = await createStore(resolver, fetcher, {
    store,
    packageImportMethod: 'copy',
  })
  const pkgId = 'registry.npmjs.org/is-positive/1.0.0'
  const fetchResult = await storeController.fetchPackage({
    force: false,
    verifyStoreIntegrity: true,
    pkgId,
    prefix: tempy.directory(),
    resolution: {
      integrity: 'sha1-iACYVrZKLx632LsBeUGEJK4EUss=',
      registry: 'https://registry.npmjs.org/',
      tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
    }
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
